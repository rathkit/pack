// Bundles a driver for every package in a node_modules directory and compares it with Node.
//   node tools/diff-packages.mjs /path/to/project [limit]
// For each package a driver `import * as m from 'pkg'` prints the sorted export names and their
// types. It runs once as written and once bundled; stdout and exit status must match. Resolution
// is set to Node's (the `node` condition, `main` only) so both sides load the same files.
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync, writeFileSync, mkdirSync, rmSync, existsSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = join(root, 'target', 'release', 'rathkit-pack');
const project = process.argv[2];
const limit = Number(process.argv[3] ?? 1e9);
const modules = join(project, 'node_modules');
const names = [];
for (const entry of readdirSync(modules)) {
  if (entry.startsWith('.')) continue;
  if (entry.startsWith('@')) for (const sub of readdirSync(join(modules, entry))) names.push(`${entry}/${sub}`);
  else names.push(entry);
}
const work = join(project, '.rathkit-drivers');
rmSync(work, { recursive: true, force: true });
mkdirSync(work);
const counts = { same: 0, differ: 0, bothFail: 0, bundlerFails: 0, bundleRunFails: 0 };
const shown = { differ: 0, bundlerFails: 0, bundleRunFails: 0 };
const reasons = new Map();
// Node finds a CommonJS package's named exports by static analysis and adds a `module.exports`
// key; a bundle reads the real object. So the bundle must provide every name Node reports with
// the same type, and may have more.
const missing = (got, expected) => {
  const have = new Map(got.trim().split(' ').map((p) => p.split(/:(?=[^:]*$)/)));
  return expected.trim().split(' ').filter((p) => { const [k, t] = p.split(/:(?=[^:]*$)/); return k !== 'module.exports' && t !== 'undefined' && have.get(k) !== t; }).map((p) => `${p} (bundle: ${have.get(p.split(/:(?=[^:]*$)/)[0])})`);
};
const covers = (got, expected) => missing(got, expected).length === 0;
const run = (file) => {
  const r = spawnSync('node', ['--no-warnings', file], { cwd: project, encoding: 'utf8', timeout: 30000, maxBuffer: 1 << 28 });
  return { out: r.stdout ?? '', err: (r.stderr ?? '').split('\n').find((l) => /Error|error/.test(l)) ?? '', code: r.status };
};
for (const name of names.slice(0, limit)) {
  const driver = join(work, name.replace('/', '__') + '.mjs');
  writeFileSync(driver, `import * as m from ${JSON.stringify(name)};\nconsole.log(Object.keys(m).sort().map((k) => { try { return k + ':' + typeof m[k]; } catch (e) { return k + ':throws'; } }).join(' '));\n`);
  const expected = run(driver);
  if (expected.code !== 0) { counts.bothFail++; continue; }   // not importable in plain Node
  const out = driver.replace(/\.mjs$/, '.bundle.mjs');
  const b = spawnSync(bin, ['bundle', driver, '--outfile', out, '--condition', 'node', '--main-field', 'main', '--original-paths', ...(process.env.EXTRA_FLAGS ? process.env.EXTRA_FLAGS.split(' ') : [])], { cwd: project, encoding: 'utf8', timeout: 60000 });
  if (b.status !== 0) {
    counts.bundlerFails++;
    const why = (b.stderr.split('\n').find((l) => /error/.test(l)) ?? b.stderr.slice(0, 120)).replace(/^.*?error: /, '').replace(/'\.[^']*'/g, (m) => m).slice(0, 90);
    reasons.set(why, (reasons.get(why) ?? 0) + 1);
    if (shown.bundlerFails++ < 12) console.log(`bundler failed: ${name}: ${b.stderr.split('\n').find((l) => /error/.test(l))?.slice(0, 200)}`);
    continue;
  }
  const got = run(out);
  if (got.code !== 0) {
    counts.bundleRunFails++;
    if (shown.bundleRunFails++ < 12) console.log(`bundle failed to run: ${name}: ${got.err.slice(0, 200)}`);
  } else if (!covers(got.out, expected.out)) {
    counts.differ++;
    if (shown.differ++ < 12) console.log(`DIFFERENT: ${name}: ${missing(got.out, expected.out).slice(0, 5).join(', ')}`);
  } else counts.same++;
}
rmSync(work, { recursive: true, force: true });
console.log(counts);
if (reasons.size) console.log('bundler failure reasons:', [...reasons].sort((a, b) => b[1] - a[1]).slice(0, 8));
