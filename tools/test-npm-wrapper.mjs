// The npm packages work as installed: the launcher finds this machine's platform package, passes arguments
// and the exit code through, and no package has an install script or a dependency but the optional ones.
//   node tools/test-npm-wrapper.mjs
import { mkdtempSync, mkdirSync, cpSync, readFileSync, writeFileSync, rmSync, readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const tmp = mkdtempSync(join(tmpdir(), 'npm-wrapper-'));
const made = join(tmp, 'made');
const run = (cmd, args, options = {}) => spawnSync(cmd, args, { encoding: 'utf8', ...options });
const fail = (message) => {
  console.log(`FAIL npm wrapper: ${message}`);
  process.exit(1);
};

const m = run(process.execPath, [join(root, 'tools', 'make-npm-packages.mjs'), '--out', made, '--local']);
if (m.status !== 0) fail(`cannot make the packages: ${m.stderr}${m.stdout}`);

// The old name of the command is another bin of the launcher.
const bins = JSON.parse(readFileSync(join(made, 'pack', 'package.json'), 'utf8')).bin;
if (bins.rkp !== 'bin/rkp.js' || bins['rathkit-pack'] !== 'bin/rkp.js') fail(`the launcher's bins are ${JSON.stringify(bins)}`);

// What each package.json says.
for (const d of readdirSync(made)) {
  const manifest = JSON.parse(readFileSync(join(made, d, 'package.json'), 'utf8'));
  for (const key of ['dependencies', 'devDependencies', 'peerDependencies']) if (manifest[key] && Object.keys(manifest[key]).length) fail(`${manifest.name} has ${key}`);
  for (const key of ['preinstall', 'install', 'postinstall', 'prepare', 'prepublish', 'prepack']) if (manifest.scripts?.[key]) fail(`${manifest.name} has an install-time script: ${key}`);
  if (manifest.scripts) fail(`${manifest.name} has scripts`);
}

// Installed as npm lays them out: a scope folder in node_modules.
const project = join(tmp, 'project');
const scope = join(project, 'node_modules', '@rathkit');
mkdirSync(scope, { recursive: true });
for (const d of readdirSync(made)) cpSync(join(made, d), join(scope, d), { recursive: true });
const launcher = join(scope, 'pack', 'bin', 'rkp.js');
const version = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root, 'Cargo.toml'), 'utf8'))[1];

const v = run(process.execPath, [launcher, '--version'], { cwd: project });
if (v.stdout.trim() !== `rkp ${version}`) fail(`--version printed ${JSON.stringify(v.stdout)} ${v.stderr}`);

writeFileSync(join(project, 'main.js'), 'console.log("hello " + (1 + 2));\n');
const b = run(process.execPath, [launcher, 'bundle', 'main.js', '--outfile', 'dist/out.mjs'], { cwd: project });
const r = b.status === 0 ? run(process.execPath, [join(project, 'dist', 'out.mjs')]) : b;
if (r.stdout.trim() !== 'hello 3') fail(`the bundle printed ${JSON.stringify(r.stdout)} ${b.stderr}`);

// The exit code of the program comes through.
const bad = run(process.execPath, [launcher, 'bundle', 'missing.js', '--outfile', 'dist/x.mjs'], { cwd: project });
if (bad.status === 0) fail('a failing build exited with 0');

// A platform with no package says so.
const lonely = join(tmp, 'lonely');
mkdirSync(join(lonely, 'bin'), { recursive: true });
cpSync(launcher, join(lonely, 'bin', 'rkp.js'));
const none = run(process.execPath, [join(lonely, 'bin', 'rkp.js'), '--version'], { cwd: lonely });
if (none.status === 0 || !/there is no build for/.test(none.stderr)) fail(`a missing platform package gave ${none.status} ${none.stderr}`);

console.log('npm wrapper: packages are plain, the launcher starts the program, passes arguments and exit codes, and explains a missing build');
rmSync(tmp, { recursive: true, force: true });
