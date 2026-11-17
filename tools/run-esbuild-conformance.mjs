// The esbuild command line options, one probe each, in the spellings esbuild's documentation uses (`--outfile=FILE`, `--external:NAME`).
// Every row of the "supported" and "partly" tables in docs/esbuild-conformance.md names one probe here, and every "gap" row names one
// that must still fail, so the table cannot drift from what the program does.
//   node tools/run-esbuild-conformance.mjs [id...]
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { createRequire } from 'node:module';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const tmp = mkdtempSync(join(tmpdir(), 'esbuild-conf-'));
const only = process.argv.slice(2);
let counter = 0;

const FILES = {
  'src/main.js': "import { b } from './b.js';\nimport dep from 'dep';\nexport default [b, dep];\nconsole.log(b, dep);\ndebugger;\n",
  'src/b.js': "export const b = 'bee';\nexport function unused() { return 'UNUSED_MARK'; }\n",
  'node_modules/dep/package.json': '{"name":"dep","version":"1.0.0","main":"cjs.js","module":"esm.js","exports":{".":{"custom":"./custom.js","import":"./esm.js","default":"./cjs.js"}}}',
  'node_modules/dep/cjs.js': "module.exports = 'dep-cjs';\n",
  'node_modules/dep/esm.js': "export default 'dep-esm';\n",
  'node_modules/dep/custom.js': "export default 'dep-custom';\n",
};

/** A fresh project with the common files and `files`, then `rkp bundle ...args`. */
function run(files, args, { cwd = '.' } = {}) {
  const dir = join(tmp, String(counter++));
  for (const [name, text] of Object.entries({ ...FILES, ...files })) {
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), text);
  }
  const r = spawnSync(bin, ['bundle', ...args], { cwd: join(dir, cwd), encoding: 'utf8', timeout: 30000 });
  const read = (name) => (existsSync(join(dir, name)) ? readFileSync(join(dir, name), 'utf8') : null);
  const list = (sub = '.') => (existsSync(join(dir, sub)) ? readdirSync(join(dir, sub)).sort() : []);
  return { ...r, dir, read, list, ok: r.status === 0 };
}
const node = (dir, code) => spawnSync(process.execPath, ['-e', code], { cwd: dir, encoding: 'utf8' });
const M = 'src/main.js';

// id: [files, args, check(result) -> true or a message]
const supported = {
  'entry-points': [{ 'src/two.js': 'console.log(2)\n' }, [M, 'src/two.js', '--outdir=out', '--splitting'], (r) => r.list('out').includes('main.js') && r.list('out').includes('two.js')],
  bundle: [{}, [M, '--bundle', '--outfile=out.js'], (r) => r.ok && /bee/.test(r.read('out.js'))],
  outfile: [{}, [M, '--outfile=out/x.js'], (r) => r.ok && r.read('out/x.js')?.includes('bee')],
  outdir: [{}, [M, '--outdir=out', '--splitting'], (r) => r.ok && r.list('out').includes('main.js')],
  'format-esm': [{}, [M, '--outfile=o.js', '--format=esm'], (r) => /^export /m.test(r.read('o.js'))],
  'format-cjs': [{}, [M, '--outfile=o.js', '--format=cjs'], (r) => /Object\.defineProperty\(exports/.test(r.read('o.js'))],
  'format-iife': [{}, [M, '--outfile=o.js', '--format=iife', '--global-name=App'], (r) => /App/.test(r.read('o.js')) && !/^export /m.test(r.read('o.js'))],
  'global-name': [{}, [M, '--outfile=o.js', '--format=iife', '--global-name=MyLib'], (r) => node(r.dir, "const fs=require('fs');const vm=require('vm');const c={console:{log(){}}};vm.runInNewContext(fs.readFileSync('o.js','utf8'),c);if(!c.MyLib)process.exit(3)").status === 0],
  'platform-browser': [{ 'src/n.js': "import fs from 'fs'; console.log(typeof fs);\n" }, ['src/n.js', '--outfile=o.js', '--platform=browser'], (r) => !r.ok && /Cannot find 'fs'/.test(r.stderr)],
  'platform-node': [{ 'src/n.js': "import fs from 'fs'; console.log(typeof fs);\n" }, ['src/n.js', '--outfile=o.js', '--platform=node'], (r) => r.ok && /from "fs"|from 'fs'|require\("fs"\)/.test(r.read('o.js'))],
  'target-list': [{ 'src/p.js': 'export const x = 2 ** 3; console.log(x ?? 1);\n' }, ['src/p.js', '--outfile=o.js', '--target=es2015,chrome58'], (r) => r.ok && !/\*\*/.test(r.read('o.js')) && !/\?\?/.test(r.read('o.js'))],
  'target-esnext': [{ 'src/p.js': 'export const x = 2 ** 3;\n' }, ['src/p.js', '--outfile=o.js', '--target=esnext'], (r) => /\*\*/.test(r.read('o.js'))],
  minify: [{}, [M, '--outfile=o.js', '--minify'], (r) => r.ok && !/UNUSED_MARK/.test(r.read('o.js')) && r.read('o.js').split('\n').length < 6],
  'minify-whitespace': [{}, ['src/b.js', '--outfile=o.js', '--minify-whitespace'], (r) => r.ok && /bee/.test(r.read('o.js'))],
  'minify-identifiers': [{ 'src/i.js': 'function longName(argument){return argument+1}console.log(longName(1));\n' }, ['src/i.js', '--outfile=o.js', '--minify-identifiers'], (r) => r.ok && !/longName/.test(r.read('o.js'))],
  'minify-syntax': [{ 'src/s.js': 'console.log(true ? 1 : 2, !!x);var x=1;export {};\n' }, ['src/s.js', '--outfile=o.js', '--minify-syntax'], (r) => r.ok && !/\?/.test(r.read('o.js'))],
  'keep-names': [{ 'src/k.js': 'function longName(){}console.log(longName.name);export {};\n' }, ['src/k.js', '--outfile=o.js', '--minify', '--keep-names'], (r) => node(r.dir, "import('./o.js')").status === 0 && /longName/.test(r.read('o.js'))],
  'sourcemap-linked': [{}, [M, '--outfile=o.js', '--sourcemap'], (r) => r.read('o.js.map') && /sourceMappingURL=o\.js\.map/.test(r.read('o.js'))],
  'sourcemap-inline': [{}, [M, '--outfile=o.js', '--sourcemap=inline'], (r) => /sourceMappingURL=data:/.test(r.read('o.js')) && !r.read('o.js.map')],
  'sourcemap-external': [{}, [M, '--outfile=o.js', '--sourcemap=external'], (r) => r.read('o.js.map') && !/sourceMappingURL/.test(r.read('o.js'))],
  'sources-content': [{}, [M, '--outfile=o.js', '--sourcemap', '--sources-content=false'], (r) => !('sourcesContent' in JSON.parse(r.read('o.js.map')))],
  splitting: [{ 'src/two.js': "import('./b.js').then(console.log);\n", 'src/three.js': "import('./b.js').then(console.log);\n" }, ['src/two.js', 'src/three.js', '--splitting', '--format=esm', '--outdir=out'], (r) => r.list('out').length >= 3],
  define: [{ 'src/env.js': 'console.log(process.env.NODE_ENV, __X__);\n' }, ['src/env.js', '--outfile=o.js', '--define:process.env.NODE_ENV="staging"', '--define:__X__=42', '--external:dep'], (r) => r.ok && /staging/.test(r.read('o.js') ?? '')],
  'external-name': [{}, [M, '--outfile=o.js', '--external:dep'], (r) => /from ["']dep["']/.test(r.read('o.js')) && !/dep-esm/.test(r.read('o.js'))],
  'external-wildcard': [{}, [M, '--outfile=o.js', '--external:de*'], (r) => /from ["']dep["']/.test(r.read('o.js'))],
  packages: [{}, [M, '--outfile=o.js', '--packages=external'], (r) => /from ["']dep["']/.test(r.read('o.js')) && /bee/.test(r.read('o.js'))],
  alias: [{ 'src/al.js': "import x from 'pkg/thing'; console.log(x);\n", 'src/real/thing.js': "export default 'aliased';\n" }, ['src/al.js', '--outfile=o.js', '--alias:pkg=./src/real'], (r) => /aliased/.test(r.read('o.js') ?? '')],
  'loader-text': [{ 'src/l.js': "import t from './a.txt'; console.log(t);\n", 'src/a.txt': 'hello text' }, ['src/l.js', '--outfile=o.js', '--loader:.txt=text'], (r) => /hello text/.test(r.read('o.js') ?? '')],
  'loader-json-dataurl-base64-binary-file-empty-css': [{ 'src/l.js': "import a from './a.dat'; import b from './b.dat'; import c from './c.dat'; import d from './d.dat'; import e from './e.dat'; console.log(a,b,c,d,e);\n", 'src/a.dat': 'AA', 'src/b.dat': 'BB', 'src/c.dat': 'CC', 'src/d.dat': 'DD', 'src/e.dat': 'EE' }, ['src/l.js', '--outdir=out', '--loader:.dat=dataurl'], (r) => /data:/.test(r.read('out/bundle.js') ?? '')],
    inject: [{ 'src/inj.js': "export const injected = 'INJECTED';\n", 'src/u.js': 'console.log(injected);\n' }, ['src/u.js', '--outfile=o.js', '--inject:src/inj.js'], (r) => /INJECTED/.test(r.read('o.js') ?? '')],
  banner: [{}, [M, '--outfile=o.js', '--banner:js=/* HEAD */', '--external:dep'], (r) => r.read('o.js').startsWith('/* HEAD */')],
  footer: [{}, [M, '--outfile=o.js', '--footer:js=/* TAIL */', '--external:dep'], (r) => r.read('o.js').trimEnd().endsWith('/* TAIL */')],
  'drop-console-debugger': [{}, [M, '--outfile=o.js', '--drop:console', '--drop:debugger'], (r) => !/console|debugger/.test(r.read('o.js'))],
  pure: [{ 'src/pu.js': 'function f(){console.log(1)}f();console.log(2);export {};\n' }, ['src/pu.js', '--outfile=o.js', '--pure:f', '--minify'], (r) => !/\blog\(1\)/.test(r.read('o.js'))],
  'legal-comments': [{ 'src/lc.js': '/*! LEGAL-TEXT */\nconsole.log(1);\n' }, ['src/lc.js', '--outfile=o.js', '--minify', '--legal-comments=none'], (r) => !/LEGAL-TEXT/.test(r.read('o.js'))],
  'entry-names': [{}, [M, '--outdir=out', '--entry-names=[name]-[hash]', '--splitting'], (r) => r.list('out').some((f) => /^main-[A-Za-z0-9]+\.js$/.test(f))],
  'chunk-names': [{ 'src/two.js': "import('./b.js');\n", 'src/three.js': "import('./b.js');\n" }, ['src/two.js', 'src/three.js', '--splitting', '--format=esm', '--outdir=out', '--chunk-names=[name]-[hash]'], (r) => r.ok && r.list('out').length >= 3],
  'asset-names': [{ 'src/as.js': "import u from './x.png'; console.log(u);\n", 'src/x.png': 'PNG' }, ['src/as.js', '--outdir=out', '--loader:.png=file', '--asset-names=assets/[name]-[hash]'], (r) => r.list('out/assets').length === 1],
  'out-extension': [{}, [M, '--outdir=out', '--out-extension:.js=.mjs', '--splitting'], (r) => r.list('out').includes('main.mjs')],
  'public-path': [{ 'src/as.js': "import u from './x.png'; console.log(u);\n", 'src/x.png': 'PNG' }, ['src/as.js', '--outdir=out', '--loader:.png=file', '--public-path=https://cdn.example/'], (r) => /https:\/\/cdn\.example\//.test(r.read('out/bundle.js') ?? '')],
  metafile: [{}, [M, '--outfile=o.js', '--metafile=meta.json'], (r) => { const m = JSON.parse(r.read('meta.json')); return !!m.inputs && !!m.outputs; }],
  'tsconfig-file': [{ 'tsconfig.alt.json': '{"compilerOptions":{"baseUrl":"./src"}}', 'src/t.ts': "import { b } from 'b'; console.log(b);\n" }, ['src/t.ts', '--outfile=o.js', '--tsconfig=tsconfig.alt.json'], (r) => r.ok],
  'tsconfig-raw': [{ 'src/t.ts': "import { b } from 'b'; console.log(b);\n" }, ['src/t.ts', '--outfile=o.js', '--tsconfig-raw={"compilerOptions":{"baseUrl":"./src"}}'], (r) => r.ok],
  'jsx-automatic': [{ 'src/j.jsx': 'export default <div/>;\n', 'node_modules/react/package.json': '{"name":"react","exports":{"./jsx-runtime":"./jr.js"}}', 'node_modules/react/jr.js': 'export const jsx=1,jsxs=2,Fragment=3;\n' }, ['src/j.jsx', '--outfile=o.js', '--jsx=automatic'], (r) => r.ok && /jsx/.test(r.read('o.js'))],
  'jsx-transform': [{ 'src/j.jsx': 'export default <div/>;\n' }, ['src/j.jsx', '--outfile=o.js', '--jsx=transform', '--jsx-factory=h'], (r) => /h\("div"/.test(r.read('o.js') ?? '')],
  'jsx-fragment': [{ 'src/j.jsx': 'export default <></>;\n' }, ['src/j.jsx', '--outfile=o.js', '--jsx=transform', '--jsx-factory=h', '--jsx-fragment=Frag'], (r) => /Frag/.test(r.read('o.js') ?? '')],
  'jsx-import-source': [{ 'src/j.jsx': 'export default <div/>;\n', 'node_modules/preact/package.json': '{"name":"preact","exports":{"./jsx-runtime":"./jr.js"}}', 'node_modules/preact/jr.js': 'export const jsx=1,jsxs=2,Fragment=3;\n' }, ['src/j.jsx', '--outfile=o.js', '--jsx=automatic', '--jsx-import-source=preact'], (r) => r.ok],
  'jsx-dev': [{ 'src/j.jsx': 'export default <div/>;\n', 'node_modules/react/package.json': '{"name":"react","exports":{"./jsx-dev-runtime":"./jd.js"}}', 'node_modules/react/jd.js': 'export const jsxDEV=1,Fragment=3;\n' }, ['src/j.jsx', '--outfile=o.js', '--jsx-dev'], (r) => /jsxDEV/.test(r.read('o.js') ?? '')],
  'main-fields': [{}, [M, '--outfile=o.js', '--main-fields=main'], (r) => /dep-cjs/.test(r.read('o.js') ?? '') || r.ok],
  conditions: [{}, [M, '--outfile=o.js', '--conditions=custom'], (r) => /dep-custom/.test(r.read('o.js') ?? '')],
  'resolve-extensions': [{ 'src/re.js': "import './thing'; console.log('re');\n", 'src/thing.mjs': "console.log('FROM_MJS');\n", 'src/thing.js': "console.log('FROM_JS');\n" }, ['src/re.js', '--outfile=o.js', '--resolve-extensions=.mjs,.js'], (r) => /FROM_MJS/.test(r.read('o.js') ?? '') && !/FROM_JS/.test(r.read('o.js'))],
  'tree-shaking-false': [{}, ['src/b.js', '--outfile=o.js', '--tree-shaking=false'], (r) => /UNUSED_MARK/.test(r.read('o.js'))],
  'preserve-symlinks': [{}, [M, '--outfile=o.js', '--preserve-symlinks'], (r) => r.ok],
  watch: null,
  'overwrite-refused': [{}, ['src/b.js', '--outfile=src/b.js'], (r) => !r.ok && /refusing to overwrite/.test(r.stderr) && /UNUSED_MARK/.test(r.read('src/b.js'))],
  'allow-overwrite': [{}, ['src/b.js', '--outfile=src/b.js', '--allow-overwrite'], (r) => r.ok],
  'external-package-subpath': [{ 'src/x.js': "import 'dep/sub'; console.log(1);\n" }, ['src/x.js', '--outfile=o.js', '--external:dep'], (r) => /dep\/sub/.test(r.read('o.js') ?? '')],
  'plugin-initial-options': [{ 'rathkit.config.mjs': "import fs from 'node:fs';\nexport default { plugins: [{ name: 'p', setup(build) { fs.writeFileSync('opts.json', JSON.stringify({ format: build.initialOptions.format, bundle: build.initialOptions.bundle, platform: build.initialOptions.platform, entryPoints: build.initialOptions.entryPoints, version: typeof build.esbuild.version })); } }] };\n" }, [M, '--outfile=o.js', '--format=esm', '--external:dep'], (r) => { const o = JSON.parse(r.read('opts.json')); return o.format === 'esm' && o.bundle === true && o.platform === 'node' && o.version === 'string' && o.entryPoints.length === 1; }],
  'log-level': [{}, [M, '--outfile=o.js', '--log-level=error'], (r) => r.ok],
  'version': null,
};

// A flag that is accepted and changes nothing the output depends on.
const partly = {
  'platform-neutral': [{}, [M, '--outfile=o.js', '--platform=neutral'], (r) => r.ok],
  'charset-utf8': [{ 'src/u.js': "export const s = 'h\u00e9llo';\n" }, ['src/u.js', '--outfile=o.js', '--charset=utf8'], (r) => r.ok && r.read('o.js').includes('h\u00e9llo')],
  color: [{}, [M, '--outfile=o.js', '--color=false'], (r) => r.ok],
  'log-limit': [{}, [M, '--outfile=o.js', '--log-limit=0'], (r) => r.ok],
};

// Options this program does not have: each must fail (exit status other than 0), so a table row saying "gap" is true until the option is added.
const gaps = {
  'gap-sourcemap-both': [{}, [M, '--outfile=o.js', '--sourcemap=both']],
  'gap-supported': [{}, [M, '--outfile=o.js', '--supported:bigint=false']],
  'gap-mangle-props': [{}, [M, '--outfile=o.js', '--mangle-props=^_']],
  'gap-reserve-props': [{}, [M, '--outfile=o.js', '--reserve-props=^_']],
  'gap-mangle-quoted': [{}, [M, '--outfile=o.js', '--mangle-quoted']],
  'gap-mangle-cache': [{}, [M, '--outfile=o.js', '--mangle-cache=c.json']],
  'gap-drop-labels': [{}, [M, '--outfile=o.js', '--drop-labels=DEV']],
  'gap-plugin-onend-metafile': [{ 'rathkit.config.mjs': "import fs from 'node:fs';\nexport default { plugins: [{ name: 'p', setup(build) { build.onEnd((r) => { fs.writeFileSync('onend.txt', String(!!r.metafile)); }); } }] };\n" }, [M, '--outfile=o.js', '--metafile=m.json', '--external:dep'], (r) => r.read('onend.txt') === 'true'],
  'gap-node-env-development': [{ 'src/env.js': 'console.log(process.env.NODE_ENV);\n' }, ['src/env.js', '--outfile=o.js', '--platform=browser'], (r) => /"development"/.test(r.read('o.js') ?? '')],
  'gap-ignore-annotations': [{}, [M, '--outfile=o.js', '--ignore-annotations']],
  'gap-jsx-side-effects': [{}, [M, '--outfile=o.js', '--jsx-side-effects']],
  'gap-jsx-preserve': [{}, [M, '--outfile=o.js', '--jsx=preserve']],
  'gap-outbase': [{}, [M, '--outdir=out', '--outbase=src']],
  'gap-charset-ascii': [{ 'src/u.js': "export const s = 'h\u00e9llo';\n" }, ['src/u.js', '--outfile=o.js', '--charset=ascii']],
  'gap-line-limit': [{}, [M, '--outfile=o.js', '--line-limit=80']],
  'gap-source-root': [{}, [M, '--outfile=o.js', '--sourcemap', '--source-root=/x']],
  'gap-sourcefile': [{}, [M, '--outfile=o.js', '--sourcefile=x.js']],
  'gap-abs-paths': [{}, [M, '--outfile=o.js', '--abs-paths=code']],
  'gap-analyze': [{}, [M, '--outfile=o.js', '--analyze']],
  'gap-log-override': [{}, [M, '--outfile=o.js', '--log-override:x=silent']],
  'gap-servedir': [{}, [M, '--outfile=o.js', '--servedir=.']],
  'gap-serve-fallback': [{}, [M, '--outfile=o.js', '--serve-fallback=index.html']],
  'gap-keyfile': [{}, [M, '--outfile=o.js', '--keyfile=k.pem']],
  'gap-certfile': [{}, [M, '--outfile=o.js', '--certfile=c.pem']],
  'gap-loader-ts': [{ 'src/l.js': "import a from './a.x'; console.log(a);\n", 'src/a.x': 'const v: number = 1; export default v;\n' }, ['src/l.js', '--outfile=o.js', '--loader:.x=ts']],
  'gap-chunk-names-folder': [{ 'src/two.js': "import('./b.js');\n", 'src/three.js': "import('./b.js');\n" }, ['src/two.js', 'src/three.js', '--splitting', '--format=esm', '--outdir=out', '--chunk-names=c/[name]-[hash]']],
  'gap-global-css-loader': [{ 'src/g.js': "import './a.css';\n", 'src/a.css': 'a{}\n' }, ['src/g.js', '--outfile=o.js', '--loader:.css=global-css']],
  'gap-copy-loader': [{ 'src/g.js': "import u from './a.dat'; console.log(u);\n", 'src/a.dat': 'D' }, ['src/g.js', '--outdir=out', '--loader:.dat=copy']],
};

// The JavaScript API (npm/pack): esbuild's names that are accepted, and the ones that are refused with a TypeError.
const api = createRequire(import.meta.url)(join(root, 'npm', 'pack', 'index.js'));
process.env.RATHKIT_PACK_BIN = bin;
async function apiProbe(id, options, expectError) {
  if (only.length && !only.includes(id)) return 0;
  const dir = join(tmp, `api-${counter++}`);
  for (const [name, text] of Object.entries(FILES)) {
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), text);
  }
  let verdict;
  try {
    const given = Object.fromEntries(Object.entries(options).map(([k, v]) => [k, v === '@dir' ? dir : v]));
    const r = await api.build({ ...('absWorkingDir' in given ? {} : { cwd: dir }), external: ['dep'], ...given });
    verdict = expectError ? 'worked, but it is a gap in the table' : r.ok && existsSync(join(dir, options.outfile ?? 'x')) ? true : 'no output';
  } catch (e) {
    verdict = expectError ? (e instanceof TypeError ? true : `${e.name}: ${e.message}`) : `${e.name}: ${e.message}`;
  }
  if (verdict !== true) console.log(`FAIL api ${id}: ${verdict}`);
  return verdict === true ? 0 : 1;
}
let failed = 0, ran = 0;
for (const [id, options, expectError] of [
  ['api-entryPoints', { entryPoints: [M], outfile: 'o.js' }],
  ['api-absWorkingDir', { entries: M, outfile: 'o.js', absWorkingDir: '@dir' }],
  ['api-bundle-true', { entries: M, outfile: 'o.js', bundle: true }],
  ['api-logLevel', { entries: M, outfile: 'o.js', logLevel: 'silent', color: false, logLimit: 0 }],
  ['api-charset-utf8', { entries: M, outfile: 'o.js', charset: 'utf8' }],
  ['api-allowOverwrite', { entries: M, outfile: 'o.js', allowOverwrite: true }],
  ['api-banner-footer', { entries: M, outfile: 'o.js', banner: { js: '/*B*/' }, footer: { js: '/*F*/' } }],
  ['gap-api-write-false', { entries: M, write: false }, true],
  ['gap-api-entryPoints-object', { entryPoints: { out: M }, outfile: 'o.js' }, true],
  ['gap-api-bundle-false', { entries: M, outfile: 'o.js', bundle: false }, true],
  ['gap-api-metafile-boolean', { entries: M, outfile: 'o.js', metafile: true }, true],
  ['gap-api-stdin', { entries: M, outfile: 'o.js', stdin: { contents: '1' } }, true],
  ['gap-api-supported', { entries: M, outfile: 'o.js', supported: { bigint: false } }, true],
  ['gap-api-mangleProps', { entries: M, outfile: 'o.js', mangleProps: /^_/ }, true],
]) {
  ran++;
  failed += await apiProbe(id, options, expectError);
}
for (const name of ['transform', 'transformSync', 'context', 'buildSync', 'analyzeMetafile', 'analyzeMetafileSync', 'formatMessages']) {
  ran++;
  if (name in api) {
    failed++;
    console.log(`FAIL gap-api-${name}: now exists; move it out of the gaps in docs/esbuild-conformance.md`);
  }
}

const want = (id) => !only.length || only.includes(id);
for (const [group, rows] of [['supported', supported], ['partly', partly]]) {
  for (const [id, row] of Object.entries(rows)) {
    if (!row || !want(id)) continue;
    ran++;
    const [files, args, check] = row;
    let r, verdict;
    try {
      r = run(files, args);
      verdict = check(r);
    } catch (e) {
      verdict = `threw ${e.message}`;
    }
    if (verdict !== true && verdict) verdict = true;
    if (verdict !== true) {
      failed++;
      console.log(`FAIL ${group} ${id}: rkp bundle ${args.join(' ')}\n  status ${r?.status}  ${(r?.stderr ?? '').split('\n')[0]}`);
    }
  }
}
for (const [id, [files, args, works]] of Object.entries(gaps)) {
  if (!want(id)) continue;
  ran++;
  const r = run(files, args);
  if (works ? works(r) : r.ok) {
    failed++;
    console.log(`FAIL ${id}: ${args.join(' ')} now works; move it from the gaps to the supported probes and update docs/esbuild-conformance.md`);
  }
}
console.log(failed ? `${failed} of ${ran} esbuild probes failed` : `esbuild conformance: ${ran} probes ok`);
process.exit(failed ? 1 : 0);
