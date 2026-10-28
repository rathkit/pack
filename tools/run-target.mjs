// Tests for --target: each program under tests/target/ is run as written (Node has all the syntax) and bundled for each target in several
// modes, and what it prints must be the same. The bundle must also parse as the language of its target (acorn at that ecmaVersion), so
// no newer syntax is left in it, helpers included.
//   node tools/run-target.mjs [name...]
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const base = join(root, 'tests', 'target');
const bench = join(root, 'bench', 'node_modules');
let acorn = null;
try {
  acorn = createRequire(join(bench, 'x.js'))('acorn');
} catch {
  console.log('target: acorn is not installed (pnpm install --dir bench): the leftover-syntax check is skipped');
}
// The targets that lower something; `esnext` and `es2022` leave the programs as they are.
const targets = ['es2015', 'es2016', 'es2017', 'es2018', 'es2019', 'es2020', 'es2021', 'es2022', 'esnext', 'chrome80', 'safari13', 'firefox78', 'node14', 'ios14.5', 'chrome60,safari12'];
const modes = [[], ['--minify'], ['--format', 'cjs'], ['--format', 'iife']];
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));
const tmp = mkdtempSync(join(tmpdir(), 'target-'));
let failed = 0;
let builds = 0;
for (const name of names) {
  const dir = join(base, name);
  const original = spawnSync('node', ['--no-warnings', join(dir, 'main.js')], { cwd: dir, encoding: 'utf8' });
  if (original.status !== 0) {
    console.log(`FAIL ${name}: the program fails as written\n${original.stderr}`);
    failed++;
    continue;
  }
  // `targets.txt`: the targets to try, when the program uses something that only the later ones can have.
  const list = existsSync(join(dir, 'targets.txt')) ? readFileSync(join(dir, 'targets.txt'), 'utf8').split(/\s+/).filter(Boolean) : targets;
  // `modes.txt`: the modes to try, one a line (`plain` for none), when the program cannot be built in all of them (top-level await needs ESM or an IIFE).
  const modeList = existsSync(join(dir, 'modes.txt')) ? readFileSync(join(dir, 'modes.txt'), 'utf8').split('\n').map((l) => l.trim()).filter(Boolean).map((l) => (l === 'plain' ? [] : l.split(/\s+/))) : modes;
  for (const target of list) {
    for (const mode of modeList) {
      const format = mode.includes('cjs') ? 'cjs' : mode.includes('iife') ? 'iife' : 'esm';
      const out = join(tmp, `${name}-${target}-${mode.join('') || 'plain'}.${format === 'esm' ? 'mjs' : 'cjs'}`);
      const b = spawnSync(bin, ['bundle', 'main.js', '--outfile', out, '--target', target, ...mode], { cwd: dir, encoding: 'utf8' });
      builds++;
      let problem = null;
      if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
      else {
        const r = spawnSync('node', ['--no-warnings', out], { cwd: dir, encoding: 'utf8' });
        if (r.status !== 0 || r.stdout !== original.stdout) problem = `output differs\n--- expected\n${original.stdout}--- got (exit ${r.status})\n${r.stdout}${r.stderr}`;
        else if (acorn && target !== 'esnext') {
          // An engine target keeps what the engine has, which may be newer than the syntax it lacks; only a year is checked strictly.
          const year = /^es\d+$/.test(target) ? Number(target.slice(2)) : 'latest';
          try {
            acorn.parse(readFileSync(out, 'utf8'), { ecmaVersion: year, sourceType: format === 'esm' ? 'module' : 'script' });
          } catch (e) {
            problem = `the bundle is not ${target} syntax: ${e.message}`;
          }
        }
      }
      if (problem) {
        failed++;
        console.log(`FAIL ${name} [${target} ${mode.join(' ')}]: ${problem}`);
      }
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${builds} target builds agree`);
if (!failed) rmSync(tmp, { recursive: true, force: true });
process.exit(failed ? 1 : 0);
