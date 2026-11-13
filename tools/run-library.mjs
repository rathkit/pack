// Tests for library mode: several formats in one build, an `exports` map written for the package, one file for each module, and
// declarations (tsc from the test-deps folder for the last: pnpm install --dir test-deps).
//   node tools/run-library.mjs
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, symlinkSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { testModules } from './lib/test-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const tmp = mkdtempSync(join(tmpdir(), 'library-'));
let failed = 0;
const check = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n' + detail : ''}`);
  }
};
const fresh = () => {
  const dir = mkdtempSync(join(tmp, 'p-'));
  cpSync(join(root, 'tests', 'library'), dir, { recursive: true });
  return dir;
};
const run = (cwd, args) => spawnSync(bin, args, { cwd, encoding: 'utf8' });
const node = (cwd, code) => spawnSync(process.execPath, ['-e', code], { cwd, encoding: 'utf8' });
const files = (dir, base = '') => readdirSync(join(dir, base), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(dir, `${base}${e.name}/`) : [`${base}${e.name}`])).sort();
const NAMES = 'PI,add,count,default,legacyName,shout,text,version';
const entries = ['src/index.ts', 'src/only.ts', 'src/extra.ts'];

// Both formats of three entries: ES modules split what the entries share, each CommonJS entry stands alone.
{
  const dir = fresh();
  const r = run(dir, ['bundle', ...entries, '--format', 'esm,cjs', '--outdir', 'dist']);
  check(r.status === 0, 'a build in two formats', r.stderr);
  const out = existsSync(join(dir, 'dist')) ? files(dir, 'dist/') : [];
  for (const f of ['index', 'only', 'extra']) for (const e of ['mjs', 'cjs']) check(out.includes(`dist/${f}.${e}`), `dist/${f}.${e} is written`, out.join());
  check(out.some((f) => /chunk-.*\.mjs$/.test(f)), 'what the entries share is a chunk of the ES module build', out.join());
  const keys = (code) => node(dir, code).stdout.trim();
  check(keys("import('./dist/index.mjs').then((m) => console.log(Object.keys(m)))") === `[\n  'PI',         'add',\n  'count',      'default',\n  'legacyName', 'shout',\n  'text',       'version'\n]`, 'the ES module has the exports', keys("import('./dist/index.mjs').then((m) => console.log(Object.keys(m)))"));
  check(keys("console.log(Object.keys(require('./dist/index.cjs')).join())") === NAMES, 'the CommonJS file has the same exports');
  check(keys("console.log(require('./dist/index.cjs').default('x'))") === 'HELLO X!', 'and it runs');
  check(keys("console.log(typeof require('./dist/only.cjs'))") === 'function', 'an entry with only a default export is module.exports');
  check(keys("import('./dist/only.mjs').then((m) => console.log(Object.keys(m).join()))") === 'default', 'and is a default in the ES module');
  check(keys("import('./dist/index.cjs').then((m) => console.log(Object.keys(m).filter((k) => k !== '__esModule' && k !== 'module.exports').join()))") === NAMES, 'import of the CommonJS file finds the names Node reads from it');
  const bad = run(dir, ['bundle', 'src/index.ts', '--format', 'esm,iife', '--outdir', 'x']);
  check(bad.status !== 0 && /iife/.test(bad.stderr), 'a list with iife is refused');
  const nodir = run(dir, ['bundle', 'src/index.ts', '--format', 'esm,cjs']);
  check(nodir.status !== 0 && /--outdir or --outfile/.test(nodir.stderr), 'several formats need a place to write');
  const one = run(dir, ['bundle', 'src/only.ts', '--format', 'esm,cjs', '--outfile', 'single/lib.js']);
  check(one.status === 0 && existsSync(join(dir, 'single', 'lib.mjs')) && existsSync(join(dir, 'single', 'lib.cjs')), '--outfile with two formats writes .mjs and .cjs', one.stderr);
}

// One file for each module, in the folders the sources are in.
{
  const dir = fresh();
  const r = run(dir, ['bundle', ...entries, '--format', 'esm,cjs', '--preserve-modules', '--preserve-modules-root', 'src', '--outdir', 'dist', '--sourcemap']);
  check(r.status === 0, 'a build with --preserve-modules', r.stderr);
  const out = files(dir, 'dist/').filter((f) => !f.endsWith('.map'));
  const want = ['index', 'only', 'extra', 'lib/text', 'lib/count', 'lib/math', 'lib/data.json', '_deps/cjs-dep/index'].flatMap((f) => [`dist/${f}.mjs`, `dist/${f}.cjs`]).sort();
  check(out.join() === want.join(), 'a file for each module, named by where the source is', out.join('\n'));
  check(/from "\.\/lib\/text\.mjs"/.test(readFileSync(join(dir, 'dist', 'index.mjs'), 'utf8')), 'the imports name the files next to them');
  check(/require\("\.\/math\.cjs"\)/.test(readFileSync(join(dir, 'dist', 'lib', 'count.cjs'), 'utf8')), 'and so do the requires');
  check(!/__cjs|__ns/.test(readFileSync(join(dir, 'dist', 'lib', 'math.mjs'), 'utf8')), 'a file has only the helpers it uses');
  check(/sourceMappingURL=count\.mjs\.map/.test(readFileSync(join(dir, 'dist', 'lib', 'count.mjs'), 'utf8')) && /"\.\.\/\.\.\/src\/lib\/count\.ts"/.test(readFileSync(join(dir, 'dist', 'lib', 'count.mjs.map'), 'utf8')), 'a map in a folder is linked and names the source from there');
  const keys = (code) => node(dir, code).stdout.trim();
  check(keys("console.log(Object.keys(require('./dist/index.cjs')).join())") === NAMES, 'the CommonJS files have the same exports');
  check(keys("const m = require('./dist/index.cjs'); console.log(m.default('x'), m.count(2), m.version, m.legacyName, m.text.shout('a'))") === 'HELLO X! 3 9.9.9 cjs-dep A!', 'and run');
  check(keys("import('./dist/index.mjs').then((m) => console.log(m.default('y'), m.count(2), m.version, m.legacyName))") === 'HELLO Y! 3 9.9.9 cjs-dep', 'as do the ES modules');
  check(keys("console.log(typeof require('./dist/only.cjs'))") === 'function', 'an entry that is only a default is module.exports');
  // A module no entry reaches by name still keeps all its exports.
  check(keys("const k = Object.keys(require('./dist/lib/text.cjs')); console.log(k.includes('lower') && k.includes('shout'))") === 'true', 'a module keeps the exports nothing in the library uses');
}

// --write-exports, and the package found by its own name.
{
  const dir = fresh();
  const r = run(dir, ['bundle', ...entries, '--format', 'esm,cjs', '--outdir', 'dist', '--write-exports']);
  check(r.status === 0, 'a build with --write-exports', r.stderr);
  const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
  check(pkg.name === 'tiny-lib' && pkg.type === 'module', 'the fields it does not write are kept');
  check(JSON.stringify(Object.keys(pkg.exports)) === '[".","./only","./extra","./package.json"]', 'a subpath for each entry, index first', JSON.stringify(pkg.exports));
  check(pkg.exports['.'].import === './dist/index.mjs' && pkg.exports['.'].require === './dist/index.cjs' && pkg.main === './dist/index.cjs' && pkg.module === './dist/index.mjs', 'with import and require', JSON.stringify(pkg));
  const keys = (code) => node(dir, code).stdout.trim();
  check(keys("import('tiny-lib').then((m) => console.log(Object.keys(m).join()))") === NAMES, 'import of the package by name gets the ES module');
  check(keys("console.log(Object.keys(require('tiny-lib')).join())") === NAMES, 'require of it gets the CommonJS file');
  check(keys("console.log(typeof require('tiny-lib/only'))") === 'function', 'a subpath works');
}

// Declarations next to the files, named by the exports map.
const typescript = join(testModules, 'typescript');
if (existsSync(typescript)) {
  const dir = fresh();
  mkdirSync(join(dir, 'node_modules'), { recursive: true });
  symlinkSync(realpathSync(typescript), join(dir, 'node_modules', 'typescript'), 'dir');
  const r = run(dir, ['bundle', ...entries, '--format', 'esm,cjs', '--outdir', 'dist', '--dts-bundle', '--write-exports']);
  check(r.status === 0, 'a build with --dts-bundle in two formats', r.stderr);
  const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
  check(pkg.types === './dist/index.d.ts' && pkg.exports['.'].types === './dist/index.d.ts' && pkg.exports['./only'].types === './dist/only.d.ts', 'the exports map names the declarations', JSON.stringify(pkg.exports));
  check(/export \{[^}]*\bshout\b/.test(readFileSync(join(dir, 'dist', 'index.d.ts'), 'utf8')), 'the declarations list the exports');
} else console.log('library: typescript is not installed; the declaration checks are skipped');

rmSync(tmp, { recursive: true, force: true });
console.log(failed ? `${failed} failed` : 'library: all passed');
process.exit(failed ? 1 : 0);
