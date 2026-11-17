// The Rollup and Vite conformance suite (docs/rollup-vite-conformance.md): every row of the tables that says "works" has a
// fixture under tests/rollup-vite/<name>/ and is built here with `rkp bundle main.js --config rathkit.config.mjs`.
// A fixture has: main.js (the entry), rathkit.config.mjs (the plugins, as a Rollup or Vite config would list them),
// expected.txt (what `node dist/bundle.mjs` prints), and optionally
//   no-entry    (an empty file) the entry comes from the config's `input`, none is given on the command line
//   flags.json  extra command line flags (an array), `run.json` false to skip the run (output checked only),
//   check.mjs   default export (outdir, fixtureDir) => null | problem text, to check files the build wrote.
// Loaders and plugins are the real packages from test-deps/ (RATHKIT_TEST_DEPS names another folder with a node_modules).
//   node tools/run-rollup-vite-conformance.mjs [name...]
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, symlinkSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';
import { testModules, installHint } from './lib/test-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const base = join(root, 'tests', 'rollup-vite');
if (!existsSync(join(testModules, '@rollup', 'plugin-json'))) {
  console.log('rollup-vite-conformance: skipped (run ' + installHint + ' first)');
  process.exit(0);
}
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n))).sort();
const tmp = mkdtempSync(join(tmpdir(), 'rvc-'));
const json = (file, fallback) => (existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : fallback);
let failed = 0;
for (const name of names) {
  const src = join(base, name);
  const dir = join(tmp, name);
  cpSync(src, dir, { recursive: true });
  symlinkSync(testModules, join(dir, 'node_modules'), 'dir');
  const out = join(dir, 'dist');
  const flags = json(join(src, 'flags.json'), []);
  const b = spawnSync(bin, ['bundle', ...(existsSync(join(src, 'no-entry')) ? [] : ['main.js']), '--outfile', join(out, 'bundle.mjs'), '--config', 'rathkit.config.mjs', ...flags], { cwd: dir, encoding: 'utf8' });
  let problem = null;
  if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
  else {
    if (json(join(src, 'run.json'), true)) {
      const r = spawnSync('node', ['--no-warnings', join(out, 'bundle.mjs')], { cwd: dir, encoding: 'utf8' });
      const expected = readFileSync(join(src, 'expected.txt'), 'utf8');
      if (r.status !== 0) problem = `run failed: ${r.stderr.trim()}`;
      else if (r.stdout !== expected) problem = `printed\n${r.stdout}expected\n${expected}`;
    }
    if (!problem && existsSync(join(src, 'check.mjs'))) problem = await (await import(pathToFileURL(join(dir, 'check.mjs')).href)).default(out, dir);
  }
  console.log(problem ? `FAIL ${name}: ${problem}` : `ok   ${name}`);
  if (problem) failed++;
}
console.log(failed ? `${failed} failure(s)` : `all ${names.length} Rollup and Vite conformance builds agree`);
if (!failed) try { rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {}
else console.log('kept', tmp);
process.exit(failed ? 1 : 0);
