// The "after" side of the migration guides (docs/migrate/*.md): every fixture in tests/migrate is a small project of one tool, and the
// command (or files) the guide gives for it is run here and the program it builds is run, to see the same line the tool's own build
// prints. The guides must contain each command written here, so a guide cannot drift from what is tested.
//   node tools/run-migrate.mjs
import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, writeFileSync, existsSync, renameSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', process.platform === 'win32' ? 'rkp.exe' : 'rkp');
const tmp = mkdtempSync(join(tmpdir(), 'migrate-'));
const LINE = 'hello world 42 aliased from a text file 1.2.3 production';
let failed = 0;
const check = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n' + detail : ''}`);
  }
};

/** Words of a command line, with single quotes as a shell has them. */
function words(text) {
  return [...text.matchAll(/(?:[^\s']|'[^']*')+/g)].map((m) => m[0].replace(/'([^']*)'/g, '$1'));
}
const fresh = (name) => {
  const dir = mkdtempSync(join(tmp, `${name}-`));
  cpSync(join(root, 'tests', 'migrate', name), dir, { recursive: true });
  // The repository's secret scan refuses any file called .env, so the Vite project keeps its one as dot-env and it gets its real name here.
  if (existsSync(join(dir, 'dot-env'))) renameSync(join(dir, 'dot-env'), join(dir, '.env'));
  return dir;
};
const node = (cwd, file, args = []) => spawnSync(process.execPath, [file, ...args], { cwd, encoding: 'utf8' });
const guide = (name) => readFileSync(join(root, 'docs', 'migrate', `${name}.md`), 'utf8');
/** Runs `command` (it starts with `rkp`) in `dir`, and the guide has to show it. */
function rkp(name, dir, command) {
  check(guide(name).includes(command), `${name}: docs/migrate/${name}.md shows the command`, command);
  const r = spawnSync(bin, words(command).slice(1), { cwd: dir, encoding: 'utf8' });
  check(r.status === 0, `${name}: ${command}`, r.stderr);
  return r;
}
const stubDocument = 'globalThis.document = { createElement: () => ({ relList: { supports: () => true } }) };\nawait import(process.argv[2]);\n';

// esbuild: the command line is the same one with `rkp bundle` for `esbuild`.
{
  const dir = fresh('esbuild');
  const scripts = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).scripts;
  const before = scripts.build.replace(/^esbuild /, '');
  rkp('esbuild', dir, `rkp bundle ${before}`);
  const r = node(dir, 'dist/main.js');
  check(r.stdout.trim() === LINE, 'esbuild: the program prints the same line', r.stdout + r.stderr);
  check(existsSync(join(dir, 'dist', 'main.js.map')), 'esbuild: a source map');
}
// esbuild's JavaScript API: the package name is the only change.
{
  const dir = fresh('esbuild');
  const source = readFileSync(join(dir, 'build.mjs'), 'utf8');
  check(guide('esbuild').includes("import { build } from '@rathkit/pack';"), 'esbuild: the guide shows the import');
  writeFileSync(join(dir, 'build-rkp.mjs'), source.replace("from 'esbuild'", `from '${pathToFileURL(join(root, 'npm', 'pack', 'index.js')).href}'`));
  const r = spawnSync(process.execPath, ['build-rkp.mjs'], { cwd: dir, encoding: 'utf8', env: { ...process.env, RATHKIT_PACK_BIN: bin } });
  check(r.status === 0, 'esbuild: build() from @rathkit/pack', r.stderr);
  const out = node(dir, 'dist/main.js');
  check(out.stdout.trim() === LINE, 'esbuild: the API build prints the same line', out.stdout + out.stderr);
}
// Vite: the page is the entry, the config is read for `define` and `resolve.alias`.
{
  const dir = fresh('vite');
  rkp('vite', dir, 'rkp bundle index.html --config vite.config.js --outdir dist --minify --sourcemap');
  writeFileSync(join(dir, 'run.mjs'), stubDocument);
  const r = node(dir, 'run.mjs', [pathToFileURL(join(dir, 'dist', 'main.js')).href]);
  check(r.stdout.trim() === `${LINE} https://api.example.test`, 'vite: the program prints the same line', r.stdout + r.stderr);
  check(/<script type="module" src="\.\/main\.js">/.test(readFileSync(join(dir, 'dist', 'index.html'), 'utf8')), 'vite: the page points at the built file');
}
// webpack: the config is read for the entry, `resolve.alias`, `DefinePlugin` and `module.rules`.
{
  const dir = fresh('webpack');
  rkp('webpack', dir, 'rkp bundle src/main.js --config webpack.config.js --platform=browser --minify --sourcemap --outfile dist/main.js');
  const r = node(dir, 'dist/main.js');
  check(r.stdout.trim() === LINE, 'webpack: the program prints the same line', r.stdout + r.stderr);
}
// Rollup: its resolve, CommonJS, replace and minify plugins are what rkp does itself; the project's own plugins stay in a config file.
{
  const dir = fresh('rollup');
  cpSync(join(dir, 'after', 'rathkit.config.mjs'), join(dir, 'rathkit.config.mjs'));
  check(guide('rollup').includes(readFileSync(join(dir, 'rathkit.config.mjs'), 'utf8').trim()), 'rollup: the guide shows the config file');
  rkp('rollup', dir, "rkp bundle src/main.js --outfile dist/main.js --define:__APP_VERSION__='\"1.2.3\"' --platform=browser --minify --sourcemap");
  const r = node(dir, 'dist/main.js');
  check(r.stdout.trim() === LINE, 'rollup: the program prints the same line', r.stdout + r.stderr);
}
console.log(failed ? `${failed} migration checks failed` : 'migration guides: all commands run and agree');
process.exit(failed ? 1 : 0);
