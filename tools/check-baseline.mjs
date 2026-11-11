// Output size and peak memory against a stored baseline (roadmap 3.6): a change that makes the output bigger or the bundler use more
// memory fails, with a table of what moved.
//
//   node tools/check-baseline.mjs [--update] [--only NAME ...] [--rkp PATH] [--runs N] [--json FILE]
//
// Five workloads (test-deps/baseline/*.js: React server rendering, RxJS, all of lodash-es, three.js, the TypeScript compiler) are built
// three ways each (plain, minified, minified with a source map) with the release binary, `--no-config`, and `process.env.NODE_ENV` defined as
// "production". For each build the output bytes, the gzip bytes (level 9), the map bytes, and the peak resident memory of the rkp process
// are compared with tests/baseline.json:
//   - bytes and map may grow by at most 0.05% of the cell, gzip by 0.2% (the `tolerance` of the file): that absorbs nothing but a
//     rounding, so a change that adds even a few hundred bytes to a large bundle is named. A cell that gets smaller passes and is
//     noted: run with --update to keep the gain, so the next regression is measured from it.
//   - peak memory may grow by at most `rssPercent` (10%). It is the median of --runs (3) builds of the same input; on one machine the
//     runs agree to well under 1%, and the margin is for the difference between the machine that wrote the baseline and a shared CI
//     runner (kernel, libc, page size). The process is limited to the first four CPUs (taskset), the count of a hosted runner,
//     because the bundler starts a worker per CPU and its memory depends on how many.
// Peak memory is read with GNU time (`/usr/bin/time -f %M`), so it is measured on Linux only; elsewhere the sizes are still checked and
// memory is skipped with a note. Time is not gated here (a shared runner is too noisy): it is checked on the maintainer's machine before
// a release (docs/RELEASING.md).
// --update rewrites tests/baseline.json from this run; do it in the commit that changes the output on purpose, and say why in the message.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const BASELINE = join(root, 'tests', 'baseline.json');
export const WORKLOADS = ['react', 'rxjs', 'lodash', 'three', 'ts5'];
export const MODES = { plain: [], min: ['--minify'], 'min-map': ['--minify', '--sourcemap'] };
export const DEFAULT_TOLERANCE = { bytes: 0.0005, gzip: 0.002, map: 0.0005, rssPercent: 10 };
const CPUS = 4;

/** The peak resident set in KB from the output of `time -f %M -o FILE`: the last line, when it is a number. */
export function parseMaxRss(text) {
  const lines = String(text).split('\n').map((l) => l.trim()).filter(Boolean);
  const last = lines[lines.length - 1];
  return /^\d+$/.test(last ?? '') ? Number(last) : null;
}

export function median(values) {
  const v = [...values].sort((a, b) => a - b);
  return v.length % 2 ? v[(v.length - 1) / 2] : Math.round((v[v.length / 2 - 1] + v[v.length / 2]) / 2);
}

/**
 * Compares one measurement with its baseline. Each metric gives a row {cell, metric, was, now, change, state}; `state` is
 * "ok", "improved" (smaller by more than the tolerance: a note), or "regressed" (the failure). Memory the run did not measure
 * (not Linux) is left out; any other figure missing on one side is "regressed" (the baseline is stale).
 */
export function compareCell(cell, was, now, tolerance = DEFAULT_TOLERANCE) {
  const rows = [];
  const metrics = [['bytes', tolerance.bytes], ['gzip', tolerance.gzip], ['map', tolerance.map], ['rssKb', tolerance.rssPercent / 100]];
  for (const [metric, limit] of metrics) {
    const a = was?.[metric] ?? null;
    const b = now?.[metric] ?? null;
    if (a == null && b == null) continue;
    if (b == null && metric === 'rssKb') continue;
    if (a == null || b == null) { rows.push({ cell, metric, was: a, now: b, change: null, state: 'regressed', note: a == null ? 'not in the baseline' : 'not measured' }); continue; }
    const change = (b - a) / a;
    rows.push({ cell, metric, was: a, now: b, change, state: change > limit ? 'regressed' : change < -limit ? 'improved' : 'ok' });
  }
  return rows;
}

/** Compares a whole run with the baseline file; a cell missing on either side is a failure (the baseline is stale). */
export function compareAll(baseline, measured) {
  const rows = [];
  const tolerance = { ...DEFAULT_TOLERANCE, ...baseline.tolerance };
  for (const cell of Object.keys(measured)) {
    if (!baseline.cells[cell]) rows.push({ cell, metric: 'cell', was: null, now: null, change: null, state: 'regressed', note: 'not in the baseline' });
    else rows.push(...compareCell(cell, baseline.cells[cell], measured[cell], tolerance));
  }
  for (const cell of Object.keys(baseline.cells)) {
    if (!(cell in measured)) rows.push({ cell, metric: 'cell', was: null, now: null, change: null, state: 'regressed', note: 'in the baseline but not built' });
  }
  return rows;
}

const pct = (c) => (c == null ? '' : `${c >= 0 ? '+' : ''}${(c * 100).toFixed(2)}%`);
const num = (n) => (n == null ? '-' : n.toLocaleString('en-US'));

/** The rows that are not simply ok, as a table; `all` prints every row. Empty when there is nothing to show. */
export function renderTable(rows, all = false) {
  const shown = rows.filter((r) => all || r.state !== 'ok');
  if (!shown.length) return '';
  const head = ['cell', 'metric', 'baseline', 'now', 'change', ''];
  const body = shown.map((r) => [r.cell, r.metric, num(r.was), num(r.now), pct(r.change), r.note ?? r.state.toUpperCase()]);
  const widths = head.map((h, i) => Math.max(h.length, ...body.map((b) => b[i].length)));
  const line = (cols) => cols.map((c, i) => (i >= 2 && i <= 4 ? c.padStart(widths[i]) : c.padEnd(widths[i]))).join('  ').trimEnd();
  return [line(head), ...body.map(line)].join('\n');
}

function hasGnuTime() {
  if (process.platform !== 'linux' || !existsSync('/usr/bin/time')) return false;
  return spawnSync('/usr/bin/time', ['-f', '%M', 'true'], { encoding: 'utf8' }).status === 0;
}

/** The command prefix that limits a build to the first CPUS processors when the machine has more. */
function pinPrefix() {
  const cpus = spawnSync('nproc', { encoding: 'utf8' });
  if (cpus.status !== 0 || Number(cpus.stdout) <= CPUS) return [];
  const range = `0-${CPUS - 1}`;
  return spawnSync('taskset', ['-c', range, 'true']).status === 0 ? ['taskset', '-c', range] : [];
}

function build(rkp, workload, mode, work, runs, memory, pin) {
  const out = join(work, `${workload}-${mode}.mjs`);
  const args = ['bundle', join(root, 'test-deps', 'baseline', `${workload}.js`), '--outfile', out, '--no-config', '--define', 'process.env.NODE_ENV="production"', ...MODES[mode]];
  const rss = [];
  for (let i = 0; i < (memory ? runs : 1); i++) {
    const log = join(work, 'time.txt');
    const cmd = memory ? [...pin, '/usr/bin/time', '-f', '%M', '-o', log, rkp, ...args] : [rkp, ...args];
    const r = spawnSync(cmd[0], cmd.slice(1), { encoding: 'utf8', maxBuffer: 1 << 26 });
    if (r.status !== 0) throw new Error(`${workload} ${mode}: rkp exited with ${r.status}\n${r.stderr}`);
    if (memory) {
      const kb = parseMaxRss(existsSync(log) ? readFileSync(log, 'utf8') : '');
      if (kb != null) rss.push(kb);
    }
  }
  const js = readFileSync(out);
  const cell = { bytes: js.length, gzip: gzipSync(js, { level: 9 }).length };
  if (mode === 'min-map') cell.map = statSync(`${out}.map`).size;
  if (rss.length) cell.rssKb = median(rss);
  return cell;
}

function main() {
  const args = process.argv.slice(2);
  const flag = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
  const update = args.includes('--update') && args.splice(args.indexOf('--update'), 1);
  const rkp = flag('--rkp') ?? process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', process.platform === 'win32' ? 'rkp.exe' : 'rkp');
  const runs = Number(flag('--runs') ?? 3);
  const jsonOut = flag('--json');
  const only = [];
  for (let i; (i = args.indexOf('--only')) >= 0;) only.push(args.splice(i, 2)[1]);
  if (!existsSync(rkp)) { console.error(`rkp not found at ${rkp}; run cargo build --release or use --rkp`); process.exit(2); }
  if (!existsSync(join(root, 'test-deps', 'node_modules'))) { console.error('test-deps is not installed (run pnpm install in test-deps)'); process.exit(2); }
  if (update && only.length) { console.error('--update rewrites every cell; it cannot be combined with --only'); process.exit(2); }
  const memory = hasGnuTime();
  if (update && !memory) { console.error('refusing to write a baseline with no memory figures: run --update on Linux with GNU time'); process.exit(2); }
  const pin = memory ? pinPrefix() : [];
  const work = mkdtempSync(join(tmpdir(), 'rkp-baseline-'));
  const measured = {};
  try {
    for (const w of WORKLOADS.filter((x) => !only.length || only.includes(x))) {
      for (const mode of Object.keys(MODES)) measured[`${w}/${mode}`] = build(rkp, w, mode, work, runs, memory, pin);
    }
  } finally { rmSync(work, { recursive: true, force: true }); }
  if (!memory) console.log('note: peak memory is measured on Linux with GNU time; it is not checked here');
  if (jsonOut) writeFileSync(jsonOut, `${JSON.stringify(measured, null, 2)}\n`);

  if (update) {
    const was = existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, 'utf8')) : {};
    const file = {
      note: 'Output bytes, gzip bytes, map bytes and peak memory (KB, the first four CPUs) of rkp on the workloads in test-deps/baseline, from tools/check-baseline.mjs; rewrite with --update when the output changes on purpose.',
      tolerance: { ...DEFAULT_TOLERANCE, ...was.tolerance },
      cells: measured,
    };
    writeFileSync(BASELINE, `${JSON.stringify(file, null, 2)}\n`);
    console.log(`wrote ${BASELINE} (${Object.keys(measured).length} builds)`);
    return;
  }

  const baseline = JSON.parse(readFileSync(BASELINE, 'utf8'));
  if (only.length) for (const cell of Object.keys(baseline.cells)) if (!only.includes(cell.split('/')[0])) delete baseline.cells[cell];
  const rows = compareAll(baseline, measured);
  const bad = rows.filter((r) => r.state === 'regressed');
  const better = rows.filter((r) => r.state === 'improved');
  const table = renderTable(rows);
  if (table) console.log(table);
  console.log(`${Object.keys(measured).length} builds, ${rows.length} figures: ${bad.length} regressed, ${better.length} lower than the baseline`);
  if (better.length && !bad.length) console.log('a figure is lower than the baseline: run `node tools/check-baseline.mjs --update` and commit tests/baseline.json to keep the gain');
  if (bad.length) {
    console.log('a figure grew past its tolerance (tests/baseline.json). If the growth is wanted, run with --update and say why in the commit message.');
    process.exit(1);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
