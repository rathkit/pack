// Checks the promises the security page makes about the released packages, against the packages themselves:
// no install scripts, no dependencies but the launcher's optional platform packages, complete metadata, provenance on,
// the word "alpha" in every README of a prerelease, only the files that are listed, no use of the network outside the
// files that are meant to (the development server, federation), and a build that works with no network at all.
//   node tools/check-promises.mjs        (needs target/release/rkp)
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const problems = [];
const fail = (message) => problems.push(message);

// 1. The packages a release would publish.
const out = mkdtempSync(join(tmpdir(), 'rkp-promises-'));
const made = spawnSync(process.execPath, [join(root, 'tools', 'make-npm-packages.mjs'), '--out', out, '--local'], { encoding: 'utf8' });
if (made.status !== 0) {
  console.error(made.stderr || made.stdout);
  process.exit(1);
}
const install = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublish', 'prepublishOnly', 'prepack', 'postpack'];
const version = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root, 'Cargo.toml'), 'utf8'))[1];
const prerelease = version.includes('-');
for (const name of readdirSync(out)) {
  const dir = join(out, name);
  const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
  const at = (message) => fail(`${manifest.name}: ${message}`);
  for (const script of install) if (manifest.scripts?.[script]) at(`has a ${script} script`);
  if (manifest.scripts && Object.keys(manifest.scripts).length) at('has scripts');
  for (const table of ['dependencies', 'devDependencies', 'peerDependencies', 'bundledDependencies', 'bundleDependencies']) {
    if (manifest[table] && Object.keys(manifest[table]).length) at(`has ${table}`);
  }
  for (const [dep, range] of Object.entries(manifest.optionalDependencies ?? {})) {
    if (!/^@rathkit\/pack-/.test(dep) || range !== manifest.version) at(`optional dependency ${dep}@${range} is not a platform package at its own version`);
  }
  if (manifest.private) at('is private');
  if (manifest.version !== version) at(`version ${manifest.version} is not Cargo.toml's ${version}`);
  if (manifest.license !== 'Apache-2.0') at(`license is ${manifest.license}`);
  for (const field of ['description', 'homepage', 'bugs', 'repository', 'keywords']) if (!manifest[field]?.length && !manifest[field]?.url) at(`has no ${field}`);
  if (manifest.publishConfig?.access !== 'public') at('publishConfig.access is not public');
  if (manifest.publishConfig?.provenance !== true) at('publishConfig.provenance is not true');
  if (!manifest.os && manifest.name !== '@rathkit/pack') at('has no os field');
  for (const file of ['README.md', 'LICENSE']) if (!existsSync(join(dir, file))) at(`has no ${file}`);
  if (prerelease && !/alpha|beta/i.test(readFileSync(join(dir, 'README.md'), 'utf8'))) at('README does not say alpha or beta');
  // Only what `files` lists (and what npm always adds) is in the package.
  const listed = new Set([...(manifest.files ?? []), 'package.json']);
  for (const entry of readdirSync(dir)) if (!listed.has(entry)) at(`holds ${entry}, which is not in files`);
}

// 2. The network: only these files may reach it, each for a reason.
const allowed = {
  'src/serve/https.mjs': 'the development server, behind --https, forwards to the server on this machine',
  'src/serve/client.js': 'the page the development server serves talks to that server',
  'src/federation/runtime.js': 'a federation container loads its remotes, when configured',
  'src/federation/dts.mjs': 'federation type fetching, when remote types are configured',
  'src/graph.rs': 'text of a helper that a program using ./a.wasm?init gets; it runs in that program',
};
const pattern = /TcpStream::connect|UdpSocket|net\.connect|https?\.(request|get)\(|\bfetch\(|XMLHttpRequest|new WebSocket/;
const walk = (dir) => readdirSync(dir).flatMap((f) => (statSync(join(dir, f)).isDirectory() ? walk(join(dir, f)) : [join(dir, f)]));
for (const file of walk(join(root, 'src'))) {
  if (!/\.(rs|js|mjs)$/.test(file)) continue;
  const name = relative(root, file).split('\\').join('/');
  if (pattern.test(readFileSync(file, 'utf8')) && !allowed[name]) fail(`${name} uses the network and is not on the list in tools/check-promises.mjs`);
}

// 3. A build with no network: Linux can take it away (a new network namespace holds nothing but a loopback that is down).
const binary = join(root, 'target', 'release', process.platform === 'win32' ? 'rkp.exe' : 'rkp');
if (process.platform === 'linux' && existsSync(binary)) {
  const probe = spawnSync('unshare', ['-rn', 'true']);
  if (probe.status !== 0) console.warn('check-promises: cannot make a namespace without a network here, the offline build was skipped');
  else {
    const dir = mkdtempSync(join(tmpdir(), 'rkp-offline-'));
    const entry = join(dir, 'main.ts');
    spawnSync('sh', ['-c', `printf 'import { a } from "./a";\\nconsole.log(a + 1);\\n' > ${entry}; printf 'export const a: number = 41;\\n' > ${dir}/a.ts`]);
    const build = spawnSync('unshare', ['-rn', binary, 'bundle', entry, '--outfile', join(dir, 'out.mjs'), '--minify', '--sourcemap'], { encoding: 'utf8' });
    if (build.status !== 0) fail(`a build with no network failed: ${build.stderr || build.stdout}`);
    else {
      const run = spawnSync(process.execPath, [join(dir, 'out.mjs')], { encoding: 'utf8' });
      if (run.stdout.trim() !== '42') fail(`the offline build printed ${JSON.stringify(run.stdout)}`);
    }
    rmSync(dir, { recursive: true, force: true });
  }
}
rmSync(out, { recursive: true, force: true });

if (problems.length) {
  console.error(problems.join('\n'));
  process.exit(1);
}
console.log('promises: ok');
