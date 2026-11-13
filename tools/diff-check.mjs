// Compares the checker's diagnostics with the TypeScript compiler's (roadmap 2.12, docs/typechecker.md).
//
//   node tools/diff-check.mjs --check PATH_TO_rkp-check --ts PATH_TO_typescript_package [--seeds DIR_OR_FILE ...]
//                             [--mutants N] [--rng SEED] [--examples K] [--json FILE]
//
// The oracle is `ts.createSourceFile(name, text, Latest, true, ScriptKind.TS).parseDiagnostics` of TypeScript 5.9.3: the syntactic
// diagnostics, with the scanner's among them. Every seed file is checked as it is (the false-positive gate: the checker may report
// nothing the compiler does not) and then --mutants N times each in a mutated form (truncated, a character removed, a character or an
// edge-case snippet inserted), which is where the compiler reports something. Per code the output counts:
//   matched   the compiler reports it, with the same start and length, and so does the checker
//   missed    the compiler reports a code the checker implements, at a position the checker did not report (and did not stop before)
//   false     the checker reports something the compiler does not report at that code, start and length
//   later     codes the compiler reports that the checker's stage does not implement at all (not a miss of this stage; counted so no
//             one reads "agreement" as coverage)
//   stopped   reports of the compiler after the point where the checker stopped scanning
// TypeScript's own conformance baselines (tests/baselines) are not in the npm package, so they are not used here; see the doc.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i < 0 ? def : args[i + 1]; };
const multi = (name) => { const out = []; for (let i = 0; i < args.length; i++) if (args[i] === name) out.push(args[i + 1]); return out; };
const CHECK = resolve(opt('--check', 'target/release/rkp-check'));
const TS = opt('--ts');
const MUTANTS = Number(opt('--mutants', 0));
const EXAMPLES = Number(opt('--examples', 5));
let rng = Number(opt('--rng', 1)) >>> 0 || 1;
if (!TS) { console.error('--ts PATH (the typescript 5.9.3 package directory) is required'); process.exit(2); }
const ts = createRequire(import.meta.url)(resolve(TS));
if (ts.version !== '5.9.3' && !args.includes('--any-version')) { console.error(`expected TypeScript 5.9.3, got ${ts.version}`); process.exit(2); }

const next = () => { rng ^= rng << 13; rng >>>= 0; rng ^= rng >>> 17; rng ^= rng << 5; rng >>>= 0; return rng; };
const pick = (a) => a[next() % a.length];

const codes = new Set(spawnSync(CHECK, ['--codes'], { encoding: 'utf8' }).stdout.split('\n').map((l) => /^TS(\d+):/.exec(l)?.[1]).filter(Boolean).map(Number));

function walk(p, out) {
  const st = statSync(p);
  if (st.isDirectory()) { for (const e of readdirSync(p)) if (e !== 'node_modules' && e !== 'target' && e !== '.git') walk(join(p, e), out); }
  else if (/\.(ts|mts|cts)$/.test(p)) out.push(p);
  return out;
}
const seeds = multi('--seeds').flatMap((s) => walk(resolve(s), []));
if (seeds.length === 0) { console.error('no seed .ts files (--seeds DIR_OR_FILE)'); process.exit(2); }

const POOL = ['"', "'", '`', '/', '\\', '#', '0x', '0b', '0o', '_', '1e', '1__0', '\\u{', '\\u{110000}', '\\x', '*/', '/*', '${', '}', '{', ')', '\u0001', 'é', '1n', '1.5n', '1e3n', '3in', '0_1', '/[', '/a', '\\u12', '08', '\\1', '@', ' ', '😀', 'a/b/c', '`${', 'x = /', ') / 2 / 3'];
function mutate(text) {
  const kind = next() % 4;
  const i = next() % (text.length + 1);
  switch (kind) {
    case 0: return text.slice(0, i);
    case 1: return text.slice(0, i) + text.slice(i + 1 + (next() % 3));
    case 2: return text.slice(0, i) + pick(POOL) + text.slice(i);
    default: { const j = Math.min(text.length, i + 1 + (next() % 4)); return text.slice(0, i) + pick(POOL) + text.slice(j); }
  }
}

const dir = mkdtempSync(join(tmpdir(), 'diff-check-'));
const jobs = []; // { name, text, origin, mutated }
for (const f of seeds) {
  const text = readFileSync(f, 'utf8');
  jobs.push({ name: f, text, mutated: false });
  for (let m = 0; m < MUTANTS; m++) jobs.push({ name: f, text: mutate(text), mutated: true });
}
const stats = new Map(); // code -> { matched, missed, falsePos }
const later = new Map(); const stoppedAfter = new Map();
const ex = { missed: [], falsePos: [] };
const bump = (m, k, n = 1) => m.set(k, (m.get(k) ?? 0) + n);
const row = (c) => { if (!stats.has(c)) stats.set(c, { matched: 0, matchedUnverified: 0, missed: 0, falsePos: 0, falsePosUnverified: 0 }); return stats.get(c); };
let filesClean = 0, filesAllCovered = 0, filesExact = 0, filesStopped = 0, validFalse = 0, validFiles = 0, verifiedFalseOnSeeds = 0;

const BATCH = 200;
for (let b = 0; b < jobs.length; b += BATCH) {
  const batch = jobs.slice(b, b + BATCH);
  const paths = batch.map((j, k) => {
    const ext = /\.d\.[mc]?ts$/.test(j.name) ? j.name.replace(/^.*?(\.d\.[mc]?ts)$/, '$1') : /\.[mc]?ts$/.exec(j.name)[0];
    const p = join(dir, `f${k}${ext}`);
    writeFileSync(p, j.text);
    return p;
  });
  const r = spawnSync(CHECK, ['--json', ...paths], { encoding: 'utf8', maxBuffer: 1 << 28 });
  if (r.status !== 0 && r.status !== 1) { console.error(`checker failed (${r.status}): ${r.stderr}`); process.exit(2); }
  const lines = r.stdout.split('\n').filter(Boolean).map((l) => JSON.parse(l));
  if (lines.length !== batch.length) { console.error('checker output count mismatch'); process.exit(2); }
  batch.forEach((j, k) => {
    const ours = lines[k];
    const sf = ts.createSourceFile(paths[k], j.text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
    const oracle = sf.parseDiagnostics.map((d) => ({ code: d.code, start: d.start, length: d.length }));
    const same = (a, c) => a.code === c.code && a.start === c.start && a.length === c.length;
    const stopAt = ours.stopped ? ours.stopped.at : null;
    if (ours.stopped) filesStopped++;
    let exact = true;
    for (const o of oracle) {
      if (!codes.has(o.code)) { bump(later, o.code); exact = false; continue; }
      const hit = ours.diagnostics.find((p) => same(p, o));
      if (hit) { row(o.code)[hit.verified ? 'matched' : 'matchedUnverified']++; continue; }
      if (stopAt !== null && o.start >= stopAt) { bump(stoppedAfter, o.code); exact = false; continue; }
      row(o.code).missed++; exact = false;
      if (process.env.DUMP_MISSED) writeFileSync(join(process.env.DUMP_MISSED, `m-${ex.missed.length}-${o.code}-${o.start}.ts`), j.text);
      if (ex.missed.length < EXAMPLES * 4) ex.missed.push(`${j.name} ${j.mutated ? '(mutant)' : ''} TS${o.code}@${o.start}+${o.length}`);
    }
    for (const p of ours.diagnostics) {
      if (!oracle.some((o) => same(p, o))) {
        row(p.code)[p.verified ? 'falsePos' : 'falsePosUnverified']++; exact = false;
        if (!j.mutated) validFalse++;
        if (p.verified && !j.mutated) verifiedFalseOnSeeds++;
        if (process.env.DUMP && (p.verified || process.env.DUMP_ALL)) writeFileSync(join(process.env.DUMP, `fp-${ex.falsePos.length}-${p.code}.ts`), j.text);
        if (ex.falsePos.length < EXAMPLES * 4) { ex.falsePos.push({ name: j.name, text: j.text, d: p, mutated: j.mutated }); }
      }
    }
    if (!j.mutated) validFiles++;
    if (oracle.length === 0 && ours.diagnostics.length === 0) filesClean++;
    if (oracle.length > 0 && oracle.every((o) => codes.has(o.code))) filesAllCovered++;
    if (exact && oracle.length > 0) filesExact++;
  });
}
rmSync(dir, { recursive: true, force: true });

const total = (k) => [...stats.values()].reduce((n, s) => n + s[k], 0);
const lines = [];
lines.push(`typescript ${ts.version}; ${seeds.length} seed files, ${jobs.length} files checked (${MUTANTS} mutants each, rng ${opt('--rng', 1)}), ${jobs.length - seeds.length} mutated`);
lines.push('code   verified: matched false | unverified: matched false | missed');
for (const [c, r] of [...stats].sort((a, b) => a[0] - b[0])) lines.push(`${String(c).padEnd(6)} ${String(r.matched).padStart(16)} ${String(r.falsePos).padStart(5)} | ${String(r.matchedUnverified).padStart(20)} ${String(r.falsePosUnverified).padStart(5)} | ${String(r.missed).padStart(6)}`);
const m = total('matched'), mu = total('matchedUnverified'), x = total('missed');
lines.push(`total  ${String(m).padStart(16)} ${String(total('falsePos')).padStart(5)} | ${String(mu).padStart(20)} ${String(total('falsePosUnverified')).padStart(5)} | ${String(x).padStart(6)}`);
lines.push(`agreement on implemented codes, all reports: ${m + mu + x === 0 ? 'n/a' : ((100 * (m + mu)) / (m + mu + x)).toFixed(2) + '%'} (${m + mu} of ${m + mu + x} compiler reports)`);
const after = [...stoppedAfter.values()].reduce((a, b) => a + b, 0);
lines.push(`stricter, counting the compiler reports after a stop as misses: ${((100 * (m + mu)) / (m + mu + x + after)).toFixed(2)}% of ${m + mu + x + after} reports found, ${((100 * m) / (m + mu + x + after)).toFixed(2)}% found and verified (the ones printed by default)`);
lines.push(`false positives on the unmutated seeds: ${validFalse} in ${validFiles} files (verified ones: ${verifiedFalseOnSeeds})`);
lines.push(`files with no diagnostics from either: ${filesClean}; files where every compiler diagnostic is an implemented code: ${filesAllCovered}; files matched exactly (all diagnostics, none later or after a stop): ${filesExact}`);
lines.push(`files where the scan stopped early: ${filesStopped}`);
lines.push(`compiler diagnostics this stage does not implement (code: count): ${[...later].sort((a, b) => b[1] - a[1]).map(([c, n]) => `${c}:${n}`).join(' ') || 'none'}`);
lines.push(`compiler diagnostics after a stop (code: count): ${[...stoppedAfter].sort((a, b) => b[1] - a[1]).map(([c, n]) => `${c}:${n}`).join(' ') || 'none'}`);
for (const e of ex.missed.slice(0, EXAMPLES)) lines.push(`missed: ${e}`);
for (const e of ex.falsePos.slice(0, EXAMPLES)) lines.push(`false: ${e.name} ${e.mutated ? '(mutant) ' : ''}TS${e.d.code}@${e.d.start}+${e.d.length} near ${JSON.stringify(e.text.slice(Math.max(0, e.d.start - 30), e.d.start + 30))}`);
console.log(lines.join('\n'));
const out = opt('--json', null);
if (out) writeFileSync(out, JSON.stringify({ typescript: ts.version, seeds: seeds.length, files: jobs.length, mutants: MUTANTS, stats: Object.fromEntries(stats), later: Object.fromEntries(later), stoppedAfter: Object.fromEntries(stoppedAfter), validFalse, filesStopped }, null, 2));
process.exit(validFalse > 0 || total('falsePos') > 0 ? 1 : 0); // unverified false positives are published, not gated
