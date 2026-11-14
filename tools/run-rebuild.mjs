// A rebuild that keeps what it can must write what a build from nothing writes. For each program under tests/fixtures/ the bundler is started in
// watch mode, the entry and then one of the other files are changed, and after each change the output is compared with that of a fresh build of the
// same files. Small limits for cutting a bundle into pieces make the programs, which are small, go the way big ones do.
//   node tools/run-rebuild.mjs [name...]
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const fixtures = join(root, 'tests', 'fixtures');
const only = process.argv.slice(2);
const names = readdirSync(fixtures).filter((n) => statSync(join(fixtures, n)).isDirectory() && (!only.length || only.includes(n)));
const env = { ...process.env, RATHKIT_FRAGMENT_BYTES: '700', RATHKIT_NEST_BYTES: '400' };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tmp = mkdtempSync(join(tmpdir(), 'rebuild-'));
const modes = [[], ['--minify'], ['--minify', '--keep-names'], ['--minify', '--format', 'cjs']];
let failed = 0;
let compared = 0;

for (const name of names) {
  const dir = join(tmp, name);
  cpSync(join(fixtures, name), dir, { recursive: true });
  const entry = ['main.ts', 'main.tsx', 'main.js'].find((f) => existsSync(join(dir, f)));
  if (!entry) continue;
  const others = readdirSync(dir).filter((f) => /\.(js|mjs|ts)$/.test(f) && f !== entry && !f.endsWith('.d.ts'));
  const edits = [entry, others[0]].filter(Boolean);
  for (const flags of modes) {
    const out = join(tmp, `${name}-warm.js`);
    const fresh = join(tmp, `${name}-cold.js`);
    rmSync(out, { force: true });
    const watcher = spawn(bin, ['bundle', entry, '--outfile', out, ...flags, '--watch'], { cwd: dir, env, stdio: ['ignore', 'ignore', 'pipe'] });
    let log = '';
    watcher.stderr.on('data', (d) => (log += d));
    const rounds = () => (log.match(/^watching /gm) ?? []).length;
    const waitRound = async (n) => {
      for (let i = 0; i < 150 && rounds() < n; i++) await sleep(20);
      return rounds() >= n;
    };
    try {
      if (!(await waitRound(1))) continue;
      // A program that does not build (an expected failure) has nothing to compare.
      if (!existsSync(out)) continue;
      let round = 1;
      for (const [i, file] of edits.entries()) {
        const path = join(dir, file);
        const original = readFileSync(path, 'utf8');
        for (const step of [1, 2]) {
          writeFileSync(path, `${original}\n;globalThis.__rebuild_${i}_${step} = ${step};\n`);
          if (!(await waitRound(round + 1))) {
            // A file nothing in the program reads (found only by `require.resolve`, say) is not watched.
            if (file === entry) throw new Error(`${file}: no rebuild after the change`);
            break;
          }
          round++;
          const cold = spawnSync(bin, ['bundle', entry, '--outfile', fresh, ...flags], { cwd: dir, env, encoding: 'utf8' });
          if (cold.status !== 0) continue;
          compared++;
          if (readFileSync(out, 'utf8') !== readFileSync(fresh, 'utf8')) {
            failed++;
            console.log(`FAIL ${name} ${flags.join(' ')}: after changing ${file} the rebuilt output is not that of a fresh build`);
          }
        }
        writeFileSync(path, original);
        if (await waitRound(round + 1)) round++;
      }
    } catch (e) {
      failed++;
      console.log(`FAIL ${name} ${flags.join(' ')}: ${e.message}`);
    } finally {
      watcher.kill('SIGINT');
      await new Promise((r) => (watcher.exitCode !== null ? r() : watcher.once('exit', r)));
    }
  }
}
rmSync(tmp, { recursive: true, force: true });
if (failed) {
  console.log(`${failed} failure(s)`);
  process.exit(1);
}
console.log(`rebuild: ${compared} rebuilds of ${names.length} programs match a fresh build`);
