// Makes the npm packages of a release: the launcher (@rathkit/pack) and one package per platform that holds
// the program. The version is Cargo.toml's. No package has a dependency but the launcher's optional ones, and none
// has an install script.
//   node tools/make-npm-packages.mjs --out DIR --from TARBALLS_DIR      (the release tarballs)
//   node tools/make-npm-packages.mjs --out DIR --local                  (this machine's target/release build only)
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync, chmodSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i === -1 ? undefined : process.argv[i + 1] ?? true;
};
const out = arg('--out');
if (!out) {
  console.error('usage: make-npm-packages.mjs --out DIR (--from TARBALLS_DIR | --local)');
  process.exit(2);
}
const version = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root, 'Cargo.toml'), 'utf8'))[1];

// The release's names for a platform, and npm's.
const platforms = [
  { target: 'linux-x64-gnu', os: 'linux', cpu: 'x64', libc: 'glibc', suffix: 'gnu' },
  { target: 'linux-arm64-gnu', os: 'linux', cpu: 'arm64', libc: 'glibc', suffix: 'gnu' },
  { target: 'linux-x64-musl', os: 'linux', cpu: 'x64', libc: 'musl', suffix: 'musl' },
  { target: 'linux-arm64-musl', os: 'linux', cpu: 'arm64', libc: 'musl', suffix: 'musl' },
  { target: 'macos-arm64', os: 'darwin', cpu: 'arm64' },
  { target: 'macos-x64', os: 'darwin', cpu: 'x64' },
  { target: 'windows-x64', os: 'win32', cpu: 'x64' },
];
// Linux with glibc reports its version; musl does not.
const musl = process.platform === 'linux' && !process.report?.getReport?.().header?.glibcVersionRuntime;
const here = platforms.find((p) => p.os === process.platform && p.cpu === process.arch && (!p.libc || (p.libc === 'musl') === musl));

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const made = [];
const base = JSON.parse(readFileSync(join(root, 'npm', 'pack', 'package.json'), 'utf8'));
for (const p of platforms) {
  const exe = p.os === 'win32' ? 'rkp.exe' : 'rkp';
  let binary;
  if (arg('--local')) {
    if (p !== here) continue;
    binary = join(root, 'target', 'release', exe);
  } else {
    const tarball = readdirSync(arg('--from')).find((f) => f.endsWith(`-${p.target}.tar.gz`));
    if (!tarball) throw new Error(`no tarball for ${p.target} in ${arg('--from')}`);
    const unpack = join(out, `.unpack-${p.target}`);
    mkdirSync(unpack, { recursive: true });
    const tar = spawnSync('tar', ['-xzf', join(arg('--from'), tarball), '-C', unpack], { stdio: 'inherit' });
    if (tar.status !== 0) throw new Error(`cannot unpack ${tarball}`);
    binary = join(unpack, tarball.replace(/\.tar\.gz$/, ''), exe);
  }
  if (!existsSync(binary)) throw new Error(`missing ${binary}`);
  const id = `${p.os}-${p.cpu}${p.suffix ? `-${p.suffix}` : ''}`;
  const name = `@rathkit/pack-${id}`;
  const dir = join(out, `pack-${id}`);
  mkdirSync(join(dir, 'bin'), { recursive: true });
  cpSync(binary, join(dir, 'bin', exe));
  if (p.os !== 'win32') chmodSync(join(dir, 'bin', exe), 0o755);
  cpSync(join(root, 'LICENSE'), join(dir, 'LICENSE'));
  cpSync(join(root, 'NOTICE'), join(dir, 'NOTICE'));
  writeFileSync(join(dir, 'README.md'), `# ${name}\n\nThe rathkit-pack program for ${p.os} on ${p.cpu}${p.libc ? ` (${p.libc})` : ''}. Install \`@rathkit/pack\` instead of this package. Beta: https://github.com/rathkit/pack#readme\n`);
  const manifest = {
    name,
    version,
    description: `The rathkit-pack program for ${p.os} ${p.cpu}${p.libc ? ` (${p.libc})` : ''} (beta)`,
    license: base.license,
    repository: { ...base.repository, directory: undefined },
    homepage: base.homepage,
    bugs: base.bugs,
    keywords: base.keywords,
    os: [p.os],
    cpu: [p.cpu],
    ...(p.libc ? { libc: [p.libc] } : {}),
    files: ['bin', 'README.md', 'LICENSE', 'NOTICE'],
    publishConfig: { access: 'public', provenance: true },
  };
  writeFileSync(join(dir, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
  made.push({ name, dir });
}
// The launcher lists exactly the platform packages made.
const launcher = join(out, 'pack');
cpSync(join(root, 'npm', 'pack'), launcher, { recursive: true });
cpSync(join(root, 'LICENSE'), join(launcher, 'LICENSE'));
cpSync(join(root, 'NOTICE'), join(launcher, 'NOTICE'));
base.version = version;
base.optionalDependencies = Object.fromEntries(made.map((m) => [m.name, version]));
writeFileSync(join(launcher, 'package.json'), JSON.stringify(base, null, 2) + '\n');
for (const p of platforms) rmSync(join(out, `.unpack-${p.target}`), { recursive: true, force: true });
console.log(`npm packages for ${version}: ${[...made.map((m) => m.name), '@rathkit/pack'].join(', ')} -> ${out}`);
