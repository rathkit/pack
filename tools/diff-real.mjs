// Runs real libraries doing real work, unbundled and bundled, and compares what they print.
//   node tools/diff-real.mjs /path/to/project-with-node_modules [name...]
// Each scenario is a small ES module that uses a package and prints deterministic output. It
// runs once as written and twice bundled (tree-shaking on and off); all three must agree.
import { spawnSync } from 'node:child_process';
import { writeFileSync, mkdirSync, rmSync, existsSync } from 'node:fs';
import { join, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const project = process.argv[2];
const only = process.argv.slice(3);

const scenarios = {
  typescript: `import ts from 'typescript';
const out = ts.transpileModule('enum E { A = 1, B } class C<T> { constructor(public x: T, private y?: number) {} get v(): T { return this.x } }\\nexport const f = <T,>(a: T): T => a;\\nlet s: string = \`t\${1 + 1}\`;', { compilerOptions: { target: ts.ScriptTarget.ES2017, module: ts.ModuleKind.ESNext } });
console.log(out.outputText);
const sf = ts.createSourceFile('a.ts', 'interface I { a: number } function g(x: I) { return x.a }', ts.ScriptTarget.Latest, true);
console.log(sf.statements.map((s) => ts.SyntaxKind[s.kind]).join());`,
  terser: `import { minify } from 'terser';
const r = await minify('function add(first, second) { var unused = 1; if (false) { dead(); } return first + second + (2 * 3); }\\nconsole.log(add(1, 2));', { toplevel: true, compress: { passes: 2 }, mangle: true });
console.log(r.code);`,
  prettier: `import * as prettier from 'prettier';
console.log(await prettier.format('const   a = {b:1,c:[1,2,3],d:function(){return  1}};\\nif(a){console.log( "x" )}', { parser: 'babel', semi: true }));
console.log(await prettier.format('a{color:red;margin:0 auto}', { parser: 'css' }));`,
  acorn: `import * as acorn from 'acorn';
const ast = acorn.parse('const f = async (a, {b}) => { for await (const x of y) { label: while (1) break label } }; class A { #p = 1; static { f() } }', { ecmaVersion: 2022, sourceType: 'module' });
console.log(JSON.stringify(ast).length, acorn.version, JSON.stringify(ast).slice(0, 200));`,
  esprima: `import * as esprima from 'esprima';
console.log(JSON.stringify(esprima.parseScript('var a = 1; function f(x) { return x * 2 }', { range: true })));
console.log(esprima.tokenize('a = b + 1').map((t) => t.type + ':' + t.value).join(' '));`,
  'js-yaml': `import yaml from 'js-yaml';
const doc = yaml.load('a: 1\\nb: [x, y, {z: true}]\\nc:\\n  - 1.5\\n  - null\\n  - "str"\\n');
console.log(JSON.stringify(doc));
console.log(yaml.dump({ list: [1, 2, { k: 'v' }], text: 'multi\\nline', n: null }));`,
  json5: `import JSON5 from 'json5';
console.log(JSON.stringify(JSON5.parse("{unquoted: 'single', trailing: [1,2,], hex: 0xff, /* c */ inf: Infinity}")));
console.log(JSON5.stringify({ a: 1, 'b-c': [1, 'two'], d: undefined }));`,
  semver: `import semver from 'semver';
console.log(semver.satisfies('1.2.3', '^1.0.0 || >=3'), semver.inc('1.2.3', 'minor'), semver.coerce('v2.5').version, semver.gt('1.10.0', '1.9.9'), semver.sort(['1.0.0', '0.9.1', '1.0.0-beta']).join());
console.log(semver.validRange('>=1.2.x <2'), semver.diff('1.0.0', '2.0.0'));`,
  ajv: `import Ajv from 'ajv';
const ajv = new Ajv({ allErrors: true });
const validate = ajv.compile({ type: 'object', required: ['a'], properties: { a: { type: 'integer', minimum: 1 }, b: { type: 'array', items: { enum: ['x', 'y'] } } }, additionalProperties: false });
for (const data of [{ a: 2, b: ['x'] }, { a: 0 }, { b: ['q'], c: 1 }]) { console.log(validate(data), JSON.stringify(validate.errors?.map((e) => e.instancePath + ' ' + e.message))); }`,
  minimatch: `import { minimatch } from 'minimatch';
console.log(['a.js', 'b/c.js', 'b/c.ts', '.hidden', 'x/y/z.js'].map((f) => [minimatch(f, '**/*.js'), minimatch(f, '*.js'), minimatch(f, '{a,b}/**', { dot: true })].join('')).join());`,
  micromatch: `import micromatch from 'micromatch';
console.log(micromatch(['a.js', 'b.ts', 'c/d.js', '.e.js'], ['*.js', '!a.js']), micromatch.isMatch('foo/bar.js', 'foo/*.{js,ts}'), micromatch.makeRe('a/**/b').source);`,
  rxjs: `import { of, from, Subject, BehaviorSubject, merge, interval } from 'rxjs';
import { map, filter, reduce, mergeMap, take, scan, distinctUntilChanged, toArray } from 'rxjs/operators';
const out = [];
from([1, 2, 3, 4, 5, 6]).pipe(filter((x) => x % 2 === 0), map((x) => x * 10), scan((a, b) => a + b, 0), toArray()).subscribe((v) => out.push(JSON.stringify(v)));
of(1, 2).pipe(mergeMap((x) => of(x, x + 100)), reduce((a, b) => a + b)).subscribe((v) => out.push('reduce ' + v));
const s = new BehaviorSubject(0); s.pipe(distinctUntilChanged()).subscribe((v) => out.push('bs ' + v)); s.next(1); s.next(1); s.next(2);
await new Promise((r) => interval(1).pipe(take(3), toArray()).subscribe({ next: (v) => out.push('interval ' + v), complete: r }));
console.log(out.join('\\n'));`,
  react: `import React from 'react';
import { renderToString, renderToStaticMarkup } from 'react-dom/server';
const List = ({ items }) => React.createElement('ul', { className: 'l' }, items.map((i) => React.createElement('li', { key: i }, i)));
console.log(renderToStaticMarkup(React.createElement(List, { items: ['a', 'b', 'c'] })));
console.log(renderToString(React.createElement('div', { id: 'x' }, 'hello ', React.createElement('b', null, 'world'))));`,
  commander: `import { Command } from 'commander';
const p = new Command(); p.name('t').option('-d, --debug').option('-n, --num <n>', 'a number', parseFloat).argument('<file>');
p.exitOverride().parse(['node', 'x', '-d', '-n', '3', 'in.txt']);
console.log(JSON.stringify(p.opts()), p.args, p.helpInformation().split('\\n').length);`,
  'source-map': `import { SourceMapGenerator, SourceMapConsumer } from 'source-map';
const g = new SourceMapGenerator({ file: 'o.js' });
g.addMapping({ generated: { line: 1, column: 0 }, original: { line: 3, column: 4 }, source: 'a.js', name: 'n' });
g.addMapping({ generated: { line: 2, column: 5 }, original: { line: 7, column: 1 }, source: 'b.js' });
console.log(g.toString());
await SourceMapConsumer.with(g.toJSON(), null, (c) => console.log(JSON.stringify(c.originalPositionFor({ line: 2, column: 6 }))));`,
  '@babel/parser+generator': `import { parse } from '@babel/parser';
import generate from '@babel/generator';
const ast = parse('const f = (a, b = 2) => { return a ** b }; class X extends Y { static #z = 1; m() { return #z in this } }', { sourceType: 'module', plugins: [] });
console.log(generate.default ? generate.default(ast).code : generate(ast).code);`,
  chalk: `import chalk from 'chalk';
console.log(JSON.stringify(chalk.red.bold('x')), chalk.level);`,
  debug: `import createDebug from 'debug';
const d = createDebug('app:test'); createDebug.enable('app:*');
console.log(d.enabled, d.namespace, createDebug.humanize(65000), createDebug.names.length);`,
  glob: `import { globSync } from 'glob';
console.log(globSync('src/**/*.js', { cwd: '/nonexistent-dir' }));`,
  ms: `import ms from 'ms';
console.log(ms('2 days'), ms(60000), ms('1.5h'), ms(1500, { long: true }));`,
  'lodash.merge': `import merge from 'lodash.merge';
console.log(JSON.stringify(merge({ a: [{ b: 2 }], c: { d: 1 } }, { a: [{ e: 3 }], c: { f: 4 } })));`,
};

const work = join(project, '.rathkit-real');
rmSync(work, { recursive: true, force: true });
mkdirSync(work);
const run = (file) => {
  const r = spawnSync('node', ['--no-warnings', file], { cwd: project, encoding: 'utf8', timeout: 120000, maxBuffer: 1 << 28 });
  return { out: r.stdout ?? '', err: (r.stderr ?? '').split('\n').filter((l) => /Error/.test(l))[0] ?? '', code: r.status };
};
let passed = 0, failed = 0, skipped = 0;
for (const [name, code] of Object.entries(scenarios)) {
  if (only.length && !only.includes(name)) continue;
  const pkg = name.split('+')[0];
  if (!existsSync(join(project, 'node_modules', pkg))) { skipped++; continue; }
  const file = join(work, name.replace(/[^a-z0-9]/gi, '_') + '.mjs');
  writeFileSync(file, code);
  const expected = run(file);
  if (expected.code !== 0) { console.log(`skip ${name}: does not run in plain Node (${expected.err.slice(0, 100)})`); skipped++; continue; }
  for (const flags of [[], ['--no-treeshake'], ['--splitting'], ['--minify'], ['--minify', '--sourcemap']]) {
    const split = flags.includes('--splitting');
    const out = split ? file.replace(/\.mjs$/, '-chunks/' + basename(file).replace(/\.mjs$/, '.js')) : file.replace(/\.mjs$/, flags.length ? '.' + flags.join('').replace(/--/g, '') + '.mjs' : '.bundle.mjs');
    const t = Date.now();
    const b = spawnSync(bin, ['bundle', file, ...(split ? ['--outdir', dirname(out)] : ['--outfile', out]), '--condition', 'node', '--main-field', 'main', '--original-paths', ...flags], { cwd: project, encoding: 'utf8', timeout: 120000 });
    const took = Date.now() - t;
    if (b.status !== 0) { failed++; console.log(`FAIL ${name}: bundler: ${b.stderr.split('\\n').find((l) => /error/.test(l))?.slice(0, 200)}`); continue; }
    const got = run(out);
    if (got.out !== expected.out || got.code !== 0) {
      failed++;
      console.log(`FAIL ${name}${flags.length ? ' (no tree-shaking)' : ''}: ${got.code !== 0 ? 'bundle crashed: ' + got.err : 'output differs'}`);
      if (got.code === 0) console.log(`  expected: ${JSON.stringify(expected.out.slice(0, 300))}\\n  got:      ${JSON.stringify(got.out.slice(0, 300))}`);
    } else {
      passed++;
      if (!flags.length) console.log(`ok   ${name} (bundled in ${took} ms)`);
    }
  }
}
rmSync(work, { recursive: true, force: true });
console.log(`${passed} passed, ${failed} failed, ${skipped} skipped`);
process.exit(failed ? 1 : 0);
