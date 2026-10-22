// Tests for plugins. Each program under tests/plugins/ has a rathkit.config.mjs that lists plugins
// (Rollup, Vite, esbuild, webpack loaders); it is bundled in several output modes and run with
// Node. What it prints must equal expected.txt, and the log a plugin writes at the end (if the
// program has expected-hooks.log) must equal that file.
//   node tools/run-plugins.mjs [name...]
import { spawnSync } from 'node:child_process';
import { readdirSync, statSync, mkdtempSync, rmSync, readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rathkit-pack');
const base = join(root, 'tests', 'plugins');
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));
const tmp = mkdtempSync(join(tmpdir(), 'plugins-'));
let failed = 0, builds = 0;
for (const name of names) {
  const dir = join(base, name);
  const expected = readFileSync(join(dir, 'expected.txt'), 'utf8');
  // A program that checks source maps needs them in every build.
  let modes = name === 'source-maps' ? [['--sourcemap'], ['--sourcemap', '--minify'], ['--sourcemap', '--format', 'cjs']] : [[], ['--minify'], ['--format', 'cjs'], ['--sourcemap']];
  // A program with an esm-only file needs several output files, which only the esm format has.
  if (existsSync(join(dir, 'esm-only'))) modes = modes.filter((m) => !m.includes('cjs'));
  for (const flags of modes) {
    const out = join(tmp, `${name}-${flags.join('') || 'plain'}`);
    const file = join(out, flags.includes('cjs') ? 'bundle.cjs' : 'bundle.mjs');
    const env = { ...process.env, OUT_DIR: out, BUNDLE: file };
    const b = spawnSync(bin, ['bundle', 'main.js', '--outfile', file, ...flags], { cwd: dir, encoding: 'utf8', env });
    builds++;
    let problem = null;
    if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
    else {
      const r = spawnSync('node', ['--no-warnings', '--enable-source-maps', file], { cwd: dir, encoding: 'utf8', env });
      if (r.stdout !== expected || r.status !== 0) problem = `output differs (${flags.join(' ')})\n--- expected\n${expected}--- got (exit ${r.status})\n${r.stdout}${r.stderr}`;
      const hooks = join(dir, 'expected-hooks.log');
      if (!problem && existsSync(hooks)) {
        const written = ['hooks.log', 'esbuild.log'].map((f) => join(out, f)).find(existsSync);
        const got = written ? readFileSync(written, 'utf8') : '(no log was written)';
        if (got !== readFileSync(hooks, 'utf8')) problem = `hook log differs: ${JSON.stringify(got)} vs ${JSON.stringify(readFileSync(hooks, 'utf8'))}`;
      }
    }
    if (problem) {
      failed++;
      console.log(`FAIL ${name} [${flags.join(' ')}]: ${problem}`);
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${builds} plugin builds agree`);
if (!failed) rmSync(tmp, { recursive: true, force: true });
process.exit(failed ? 1 : 0);
