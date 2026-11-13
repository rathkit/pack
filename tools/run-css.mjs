// Tests for the style sheet pipeline with the real compilers from the test-deps folder: Sass (`.scss`, `.sass`), Less, Stylus, PostCSS (a config file
// with autoprefixer and postcss-nested) and CSS modules written in Sass. Each project under tests/css/ has no rathkit config; it is copied to a
// temporary folder next to a `node_modules` that links to test-deps, bundled in several output modes and run with Node. What it prints must equal
// expected.txt. The same cases with fake compilers, which need no download, are in tests/plugins/ (run-plugins.mjs).
//   node tools/run-css.mjs [name...]
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, symlinkSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { testModules, installHint } from './lib/test-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
if (!existsSync(join(testModules, 'sass')) || !existsSync(join(testModules, 'postcss'))) {
  console.log('css: skipped (run ' + installHint + ' first)');
  process.exit(0);
}
const base = join(root, 'tests', 'css');
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));
const tmp = mkdtempSync(join(tmpdir(), 'css-'));
let failed = 0, builds = 0;
for (const name of names) {
  const expected = readFileSync(join(base, name, 'expected.txt'), 'utf8');
  for (const flags of [[], ['--minify'], ['--format', 'cjs'], ['--sourcemap']]) {
    const dir = join(tmp, `${name}-${flags.join('') || 'plain'}`);
    cpSync(join(base, name), dir, { recursive: true });
    symlinkSync(testModules, join(dir, 'node_modules'), 'junction');
    const out = join(dir, 'out');
    const file = join(out, flags.includes('cjs') ? 'bundle.cjs' : 'bundle.mjs');
    const b = spawnSync(bin, ['bundle', 'main.js', '--outfile', file, ...flags], { cwd: dir, encoding: 'utf8' });
    builds++;
    let problem = null;
    if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
    else {
      const r = spawnSync('node', ['--no-warnings', file], { cwd: dir, encoding: 'utf8', env: { ...process.env, OUT_DIR: out } });
      if (r.stdout !== expected || r.status !== 0) problem = `output differs (${flags.join(' ')})\n--- expected\n${expected}--- got (exit ${r.status})\n${r.stdout}${r.stderr}\n${existsSync(join(out, 'bundle.css')) ? readFileSync(join(out, 'bundle.css'), 'utf8') : '(no css)'}`;
    }
    if (problem) {
      failed++;
      console.log(`FAIL ${name} [${flags.join(' ')}]: ${problem}`);
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${builds} style sheet builds agree`);
if (!failed) try { rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {}
process.exit(failed ? 1 : 0);
