// The npm packages work as installed: the launcher finds this machine's platform package, passes arguments
// and the exit code through, and no package has an install script or a dependency but the optional ones.
//   node tools/test-npm-wrapper.mjs
import { mkdtempSync, mkdirSync, cpSync, readFileSync, writeFileSync, rmSync, readdirSync, existsSync } from 'node:fs';
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

// The launcher has one bin.
const bins = JSON.parse(readFileSync(join(made, 'pack', 'package.json'), 'utf8')).bin;
if (Object.keys(bins).join() !== 'rkp' || bins.rkp !== 'bin/rkp.js') fail(`the launcher's bins are ${JSON.stringify(bins)}`);

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
cpSync(join(scope, 'pack', 'index.js'), join(lonely, 'index.js'));
const none = run(process.execPath, [join(lonely, 'bin', 'rkp.js'), '--version'], { cwd: lonely });
if (none.status === 0 || !/there is no build for/.test(none.stderr)) fail(`a missing platform package gave ${none.status} ${none.stderr}`);

// The API: the same program, started with options. It is imported as installed, from an ES module.
writeFileSync(join(project, 'package.json'), JSON.stringify({ type: 'module' }));
writeFileSync(join(project, 'api-main.js'), 'import note from "./note.txt";\nimport "./style.css";\nconsole.log(__ANSWER__, note.trim());\n');
writeFileSync(join(project, 'note.txt'), 'a note\n');
writeFileSync(join(project, 'style.css'), 'p { color: red; }\n');
writeFileSync(join(project, 'index.html'), '<!doctype html><html><head></head><body><script type="module" src="./api-main.js"></script></body></html>\n');
writeFileSync(join(project, 'remote.js'), 'export const hello = () => "remote";\n');
writeFileSync(
  join(project, 'api.mjs'),
  `import { build, serve, run, version, binaryPath, BuildError } from '@rathkit/pack';
import { readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const problems = [];
const check = (ok, what) => ok || problems.push(what);
check(typeof version() === 'string' && version().length > 0, 'version');
check(existsSync(binaryPath()), 'binaryPath');

// Options become flags: a define, a loader, minifying, a source map; the files written come back.
const one = await build({ entries: 'api-main.js', outfile: 'dist/one.mjs', define: { __ANSWER__: '42' }, loader: { '.txt': 'text' }, minify: true, sourcemap: true });
check(one.ok && one.files.some((f) => f.endsWith('one.mjs')), 'build files ' + JSON.stringify(one.files));
check(existsSync('dist/one.mjs.map'), 'a source map was asked for');
const ran = spawnSync(process.execPath, ['dist/one.mjs'], { encoding: 'utf8' });
check(ran.stdout.trim() === '42 a note', 'the output printed ' + JSON.stringify(ran.stdout));

// Options of other kinds: text around the files, dropped calls, packages left out, extensions to try.
const wrapped = await build({ metafile: 'dist/meta.json', entries: 'api-main.js', outfile: 'dist/wrapped.mjs', define: { __ANSWER__: '1' }, loader: { '.txt': 'text' }, banner: { js: '// top', css: '/* top */' }, footer: '// bottom', drop: ['console'], pure: ['unusedHelper'], packages: 'external', resolveExtensions: ['.js', 'mjs'], preserveSymlinks: true });
const wrappedText = readFileSync('dist/wrapped.mjs', 'utf8');
check(wrapped.ok && wrappedText.startsWith('// top\\n') && wrappedText.includes('// bottom') && !wrappedText.includes('console.log'), 'banner, footer and drop ' + wrappedText);
check(readFileSync('dist/wrapped.css', 'utf8').startsWith('/* top */'), 'the css banner');
check(wrapped.metafile && wrapped.metafile.inputs['api-main.js'] && Object.keys(wrapped.metafile.outputs).includes('wrapped.mjs'), 'the metafile in the result ' + JSON.stringify(wrapped.metafile && Object.keys(wrapped.metafile.outputs)));

// A page, with a folder of output.
const page = await build({ entries: 'index.html', outdir: 'dist/page' });
check(page.files.some((f) => f.endsWith('index.html')), 'page files ' + JSON.stringify(page.files));
check(/<script type="module" src="\\.\\/api-main\\.js">/.test(readFileSync('dist/page/index.html', 'utf8')), 'the page points at the built script: ' + readFileSync('dist/page/index.html', 'utf8'));

// Federation options given as an object.
const fed = await build({ entries: 'remote.js', outdir: 'dist/fed', federation: { name: 'remote', filename: 'remoteEntry.js', exposes: { './hello': './remote.js' } } });
check(existsSync('dist/fed/remoteEntry.js') && fed.ok, 'federation options as an object');

// A failed build rejects with the errors the program reported; a typo in an option is a TypeError.
try {
  await build({ entries: 'missing.js', outfile: 'dist/x.mjs' });
  check(false, 'a missing entry did not fail');
} catch (error) {
  check(error instanceof BuildError && error.errors.length > 0 && error.status !== 0, 'BuildError ' + error);
}
try {
  await build({ entries: 'api-main.js', outfile: 'dist/y.mjs', minfy: true });
  check(false, 'an unknown option was accepted');
} catch (error) {
  check(error instanceof TypeError && /minfy/.test(error.message), 'unknown option ' + error);
}
const raw = await run(['--version']);
check(raw.status === 0 && raw.stdout.startsWith('rkp '), 'run');

// The server answers and stops.
const server = await serve({ entries: 'api-main.js', port: 43917 });
const page2 = await fetch(server.url).then((r) => r.text());
check(/<script/.test(page2), 'the server served a page');
await server.close();

if (problems.length) { console.log(problems.join('\\n')); process.exit(1); }
console.log('api ok');
`,
);
const api = run(process.execPath, ['api.mjs'], { cwd: project, env: { ...process.env, RATHKIT_PACK_BIN: '' } });
if (api.status !== 0 || api.stdout.trim() !== 'api ok') fail(`the API: ${api.stdout}${api.stderr}`);

// The types fit how it is used (when a TypeScript compiler is at hand: bench has one).
const tsc = join(root, 'bench', 'node_modules', 'typescript', 'bin', 'tsc');
if (existsSync(tsc)) {
  writeFileSync(join(project, 'use.mts'), `import { build, serve, type Options, BuildError } from '@rathkit/pack';
const typed: Options = { inject: ['shim.js'], legalComments: 'linked', entryNames: '[name]-[hash]', chunkNames: 'c-[hash]', outExtension: { '.js': '.mjs' }, pure: ['a.b'], metafile: 'meta.json', entries: 'a.js', banner: { css: 'x' }, footer: 'y', drop: ['console', 'debugger'], packages: 'external', resolveExtensions: ['.ts'], preserveSymlinks: true, envDir: '.', envPrefix: ['A_'], envFiles: false };
// @ts-expect-error only console and debugger can be dropped
await build({ entries: 'a.js', drop: ['alert'] });
const options: Options = { entries: ['a.js'], outdir: 'dist', minify: { whitespace: true }, define: { A: '1' }, loader: { '.svg': 'dataurl' }, federation: { name: 'x' } };
const result = await build(options);
const files: string[] = result.files;
// @ts-expect-error a typo is not an option
await build({ entries: 'a.js', outfiel: 'x' });
// @ts-expect-error a loader that does not exist
await build({ entries: 'a.js', loader: { '.x': 'nope' } });
const server = await serve({ entries: 'a.js' });
const address: string = server.url;
void [files, address, BuildError, typed];
`);
  const t = run(process.execPath, [tsc, '--noEmit', '--strict', '--module', 'nodenext', '--moduleResolution', 'nodenext', '--target', 'es2022', '--skipLibCheck', 'false', 'use.mts'], { cwd: project });
  if (t.status !== 0) fail(`the types: ${t.stdout}${t.stderr}`);
}

console.log('npm wrapper: packages are plain, the launcher starts the program, passes arguments and exit codes, explains a missing build, and the API builds, serves and is typed');
rmSync(tmp, { recursive: true, force: true });
