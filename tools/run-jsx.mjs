// Differential test for JSX and decorators: each program under tests/jsx is compiled by the TypeScript
// compiler's own JSX transform and run with Node, then bundled and run, in both the
// automatic and the classic mode. The printed output must be identical.
//   TS_MODULE=/path/to/typescript node tools/run-jsx.mjs [name...]
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, cpSync, statSync, existsSync } from 'node:fs';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { createRequire } from 'node:module';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const require = createRequire(import.meta.url);
const ts = require(process.env.TS_MODULE ?? 'typescript');
const base = join(root, 'tests', 'jsx');
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));

const strictOf = (name) => name.endsWith('-strict');
// Programs named std-* use TC39 decorators; the others use TypeScript's legacy kind.
const legacyOf = (name) => !name.startsWith('std-');
const walk = (dir) => readdirSync(dir).flatMap((f) => (statSync(join(dir, f)).isDirectory() ? walk(join(dir, f)) : [join(dir, f)]));
const run = (file, cwd) => {
  const r = spawnSync('node', ['--no-warnings', file], { cwd, encoding: 'utf8', timeout: 30000 });
  return { out: r.stdout, err: (r.stderr.split('\n').find((l) => /Error/.test(l))) ?? '', code: r.status };
};
const tmp = mkdtempSync(join(tmpdir(), 'jsx-'));
let failed = 0;
for (const name of names) {
  const dir = join(base, name);
  for (const mode of ['automatic', 'classic']) {
    // This program's tsconfig fixes the mode.
    if (name === 'paths-and-config' && mode === 'classic') continue;
    // The reference: every source file through tsc's transform.
    const ref = join(tmp, `${name}-${mode}-ref`);
    cpSync(dir, ref, { recursive: true });
    for (const file of walk(ref)) {
      if (!/\.(tsx|ts|jsx)$/.test(file) || file.includes('node_modules')) continue;
      const out = ts.transpileModule(readFileSync(file, 'utf8'), {
        fileName: file,
        compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, experimentalDecorators: legacyOf(name), emitDecoratorMetadata: legacyOf(name), strictNullChecks: strictOf(name), jsx: mode === 'automatic' ? ts.JsxEmit.ReactJSX : ts.JsxEmit.React },
      });
      let text = out.outputText;
      // The sources import each other as `.ts`; the reference copies are `.js` files now.
      text = text.replace(/(from\s+|import\s*\(\s*)(['"])([^'"]+)\.tsx?\2/g, '$1$2$3.js$2');
      // Aliases from tsconfig `paths` are resolved by the bundler; the reference runs without them.
      if (name === 'paths-and-config') text = text.replaceAll('@components/greeting', './src/components/greeting.js').replaceAll("'@lib'", "'./src/lib/index.js'").replaceAll('src/lib/helper', './src/lib/helper.js');
      writeFileSync(file.replace(/\.(tsx|ts|jsx)$/, '.js'), text);
    }
    const expected = run('main.js', ref);
    if (expected.code !== 0 || expected.out.trim() === '') { failed++; console.log(`FAIL ${name} [${mode}]: the reference program does not run cleanly (exit ${expected.code}): ${expected.err}`); continue; }
    const entry = existsSync(join(dir, 'main.tsx')) ? 'main.tsx' : 'main.ts';
    // The standard-decorator programs also go through the other output shapes.
    const variants = name.startsWith('std-') ? [[], ['--no-treeshake'], ['--format', 'cjs'], ['--format', 'iife'], ['--minify', '--keep-names']] : [[], ['--minify', '--keep-names'], ['--minify', '--keep-names', '--sourcemap']];
    let problem = null;
    for (const variant of variants) {
      const ext = variant.includes('cjs') ? 'cjs' : 'mjs';
      const bundle = join(tmp, `${name}-${mode}${variant.join('')}.${ext}`);
      const b = spawnSync(bin, ['bundle', entry, '--outfile', bundle, '--jsx', mode, ...(legacyOf(name) ? ['--experimental-decorators', '--decorator-metadata'] : []), ...(strictOf(name) ? ['--strict-null-checks'] : []), ...variant], { cwd: dir, encoding: 'utf8' });
      if (b.status !== 0) { problem = `bundler failed ${variant.join(' ')}: ${b.stderr.trim().split('\n').slice(0, 3).join(' | ')}`; break; }
      const got = run(bundle, dir);
      if (got.out !== expected.out || got.code !== expected.code) {
        problem = `output differs ${variant.join(' ')}\n--- tsc transform (exit ${expected.code})\n${expected.out}${expected.err}\n--- bundle (exit ${got.code})\n${got.out}${got.err}\n     bundle at ${bundle}`;
        break;
      }
    }
    if (problem) { failed++; console.log(`FAIL ${name} [${mode}]: ${problem}`); }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${names.length} JSX programs agree in both modes`);
if (!failed) try { rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {} // Windows may still hold a file of a process that just ended
process.exit(failed ? 1 : 0);
