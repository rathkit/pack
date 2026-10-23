// Differential test for code splitting: each program under tests/split lists its entry points in
// entries.txt. They are bundled together into chunks, and every entry is run both as written and
// from the chunks; the printed output must be identical. An optional chunks.txt fixes the number
// of output files.
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync, existsSync, mkdtempSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rathkit-pack');
const base = join(root, 'tests', 'split');
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));
const run = (file, cwd) => {
  const r = spawnSync('node', ['--no-warnings', file], { cwd, encoding: 'utf8', timeout: 20000 });
  return { out: r.stdout, err: r.stderr.split('\n').find((l) => /Error/.test(l)) ?? '', code: r.status };
};
const tmp = mkdtempSync(join(tmpdir(), 'split-'));
let failed = 0;
for (const [name, extra] of names.flatMap((n) => [[n, []], [n, ['--minify', '--keep-names']]])) {
  const dir = join(base, name);
  const entries = readFileSync(join(dir, 'entries.txt'), 'utf8').split('\n').filter(Boolean);
  const out = join(tmp, name + extra.join(''));
  const b = spawnSync(bin, ['bundle', ...entries, '--splitting', '--outdir', out, ...extra], { cwd: dir, encoding: 'utf8' });
  if (b.status !== 0) { failed++; console.log(`FAIL ${name}${extra.join(' ')}: bundler: ${b.stderr.trim()}`); continue; }
  const files = readdirSync(out).filter((f) => f.endsWith('.js'));
  const wanted = existsSync(join(dir, 'chunks.txt')) ? Number(readFileSync(join(dir, 'chunks.txt'), 'utf8')) : null;
  if (wanted !== null && files.length !== wanted) { failed++; console.log(`FAIL ${name}${extra.join(' ')}: ${files.length} chunks, expected ${wanted}: ${files.join(' ')}`); continue; }
  // Code that only a lazy module needs must not be in what an entry loads when it starts.
  if (existsSync(join(dir, 'not-eager.txt'))) {
    const markers = readFileSync(join(dir, 'not-eager.txt'), 'utf8').split('\n').filter(Boolean);
    for (const entry of entries) {
      const seen = new Set();
      const stack = [entry];
      while (stack.length) {
        const file = stack.pop();
        if (seen.has(file) || !existsSync(join(out, file))) continue;
        seen.add(file);
        const text = readFileSync(join(out, file), 'utf8');
        for (const m of text.matchAll(/^import\b[^;]*?from\s*["']\.\/([^"']+)["']|^import\s*["']\.\/([^"']+)["']/gm)) stack.push(m[1] ?? m[2]);
      }
      for (const marker of markers) {
        const eager = [...seen].filter((f) => readFileSync(join(out, f), 'utf8').includes(marker));
        if (eager.length) { failed++; console.log(`FAIL ${name}${extra.join(' ')}/${entry}: ${marker} is loaded with the entry, in ${eager.join(', ')}`); }
      }
    }
  }
  for (const entry of entries) {
    const expected = run(entry, dir);
    if (expected.code !== 0 || expected.out.trim() === '') { failed++; console.log(`FAIL ${name}${extra.join(' ')}/${entry}: the program itself does not run cleanly: ${expected.err}`); continue; }
    const got = run(join(out, entry), dir);
    if (got.out !== expected.out || got.code !== expected.code) {
      failed++;
      console.log(`FAIL ${name}${extra.join(' ')}/${entry}: output differs\n--- expected\n${expected.out}\n--- got (exit ${got.code})\n${got.out}${got.err}\n     chunks in ${out}: ${files.join(' ')}`);
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${names.length} split programs agree`);
if (!failed) rmSync(tmp, { recursive: true, force: true });
process.exit(failed ? 1 : 0);
