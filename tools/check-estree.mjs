// Compares the ESTree this bundler writes with acorn's, over a pile of real files.
// usage: node tools/check-estree.mjs [--limit N] FILE_OR_DIR...
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = process.env.RATHKIT_PACK_BIN ?? path.join(here, '../target/release/rathkit-pack');
// acorn is the reference: ACORN_DIR names an installed copy, otherwise it is looked for from the current directory.
let found = process.env.ACORN_DIR;
if (!found) {
  try {
    found = path.dirname(createRequire(path.join(process.cwd(), 'noop.js')).resolve('acorn/package.json'));
  } catch {}
}
if (!found) {
  console.log('check-estree: acorn is not installed (set ACORN_DIR or run from a project that has it); skipped');
  process.exit(0);
}
const acorn = createRequire(path.join(found, 'noop.js'))(found);

const args = process.argv.slice(2);
let limit = Infinity;
const roots = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--limit') limit = Number(args[++i]);
  else roots.push(args[i]);
}

const files = [];
const walk = (p) => {
  let st;
  try { st = statSync(p); } catch { return; }
  if (st.isDirectory()) {
    if (path.basename(p) === '.git') return;
    for (const e of readdirSync(p)) walk(path.join(p, e));
  } else if (/\.(m?js|cjs)$/.test(p) && st.size < 400_000) files.push(p);
};
roots.forEach(walk);

// Fields acorn adds that ESTree consumers do not rely on.
const IGNORE = new Set(['range', 'loc', 'sourceFile']);
const strip = (n) => {
  if (Array.isArray(n)) return n.map(strip);
  if (n && typeof n === 'object') {
    const o = {};
    for (const k of Object.keys(n).sort()) {
      if (IGNORE.has(k)) continue;
      let v = n[k];
      if (k === 'value' && n.type === 'Literal' && (n.regex || n.bigint !== undefined)) v = null;
      if (k === 'value' && typeof v === 'number' && !Number.isFinite(v)) v = null;
      if (typeof v === 'bigint') v = null;
      o[k] = strip(v);
    }
    return o;
  }
  return n;
};

const diff = (a, b, at = '') => {
  if (JSON.stringify(a) === JSON.stringify(b)) return null;
  if (a && b && typeof a === 'object' && typeof b === 'object' && !Array.isArray(a) && !Array.isArray(b)) {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) {
      const d = diff(a[k], b[k], `${at}.${k}`);
      if (d) return d;
    }
  }
  if (Array.isArray(a) && Array.isArray(b)) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      const d = diff(a[i], b[i], `${at}[${i}]`);
      if (d) return d;
    }
  }
  const brief = (v) => (v && typeof v === 'object' ? `${v.type ?? 'array'}@${v.start}-${v.end}` : JSON.stringify(v));
  return `${at}: ours ${brief(a)} vs acorn ${brief(b)}`;
};

let checked = 0, agree = 0, skipped = 0;
const reasons = new Map();
for (const file of files) {
  if (checked >= limit) break;
  const text = readFileSync(file, 'utf8');
  let want, sourceType = 'module';
  const opts = { ecmaVersion: 'latest', ranges: false, locations: false, allowHashBang: true };
  try { want = acorn.parse(text, { ...opts, sourceType }); } catch {
    try { sourceType = 'script'; want = acorn.parse(text, { ...opts, sourceType, allowReturnOutsideFunction: true }); } catch { skipped++; continue; }
  }
  checked++;
  const run = spawnSync(exe, ['--estree', ...(sourceType === 'script' ? ['--script'] : []), '-'], { input: text, maxBuffer: 1 << 30, encoding: 'utf8' });
  let got;
  try { got = JSON.parse(run.stdout); } catch { got = { error: 'unparsable output' }; }
  if (got.error) {
    const key = `error: ${got.error}`;
    reasons.set(key, (reasons.get(key) ?? []).concat(file));
    continue;
  }
  // Both sides get the same Program range.
  want.start = 0; want.end = text.length;
  const d = diff(strip(got), strip(want));
  if (!d) { agree++; continue; }
  const key = d.replace(/\[\d+\]/g, '[]').replace(/ours .*? vs acorn/, 'ours X vs acorn').replace(/@\d+-\d+/g, '');
  const kind = d.replace(/^.*\.(\w+(\[\d+\])?): /, '$1: ');
  reasons.set(kind.replace(/@\d+-\d+/g, '').replace(/\[\d+\]/g, ''), (reasons.get(kind.replace(/@\d+-\d+/g, '').replace(/\[\d+\]/g, '')) ?? []).concat(`${file}  ${d}`));
}
console.log(`${checked} files checked, ${agree} agree, ${checked - agree} differ, ${skipped} skipped (acorn would not parse)`);
for (const [k, v] of [...reasons].sort((a, b) => b[1].length - a[1].length).slice(0, 25)) {
  console.log(`\n${v.length}x ${k}\n   ${v[0]}`);
}
process.exit(checked === agree ? 0 : 1);
