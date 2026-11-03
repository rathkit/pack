// Builds pinned open-source projects (corpus/projects.json) with rkp and checks the result.
//
//   node tools/run-corpus.mjs [name ...] [--dir DIR] [--keep] [--json FILE] [--no-browser]
//
// Each project is cloned at its pinned commit into DIR (default: a new folder under ~/.cache/rkp-corpus, not the system temp folder, which is often a small
// memory-backed volume that a hundred thousand files of node_modules fill), its dependencies are
// installed with pnpm and `--ignore-scripts`, and the `build` command line is run with rkp. A build passes when rkp exits with 0,
// the expected files exist, and, for a library, importing the output in Node gives the expected exports; an app is also loaded in headless
// Chromium (unless --no-browser, or no browser is found) and must run without uncaught errors, console errors or missing files. This runs code from
// other projects (their configuration files and plugins), so it is not part of the test battery: run it by hand or from the
// corpus workflow. The work folder is deleted when the run ends (also after a failure or Ctrl-C) unless --keep or --dir is given.
// Exit status is 1 if any project failed.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { checkPage, findBrowser } from './lib/browser-check.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); if (i < 0) return null; const [, v] = args.splice(i, 2); return v; };
const keep = args.includes('--keep') && args.splice(args.indexOf('--keep'), 1);
const given = flag('--dir');
const cache = path.join(process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache'), 'rkp-corpus');
fs.mkdirSync(cache, { recursive: true });
const dir = path.resolve(given || fs.mkdtempSync(path.join(cache, 'run-')));
const temporary = !given && !keep;
const cleanup = () => { if (temporary) try { fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {} };
process.on('exit', cleanup);
for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => process.exit(130));
const jsonOut = flag('--json');
const noBrowser = args.includes('--no-browser') && args.splice(args.indexOf('--no-browser'), 1);
const browser = noBrowser ? null : findBrowser();
const wanted = args;
const rkp = process.env.RATHKIT_PACK_BIN || path.join(root, 'target', 'release', process.platform === 'win32' ? 'rkp.exe' : 'rkp');
if (!fs.existsSync(rkp)) { console.error(`rkp not found at ${rkp}; run cargo build --release or set RATHKIT_PACK_BIN`); process.exit(2); }

const list = JSON.parse(fs.readFileSync(path.join(root, 'corpus', 'projects.json'), 'utf8')).projects.filter((p) => !wanted.length || wanted.includes(p.name));
const run = (cmd, argv, cwd, limit = 600) => {
  const r = spawnSync(cmd, argv, { cwd, encoding: 'utf8', timeout: limit * 1000, maxBuffer: 64 << 20, shell: process.platform === 'win32' });
  return { ok: r.status === 0, out: `${r.stdout || ''}${r.stderr || ''}`.trim(), ms: 0 };
};
const tail = (text, n = 12) => text.split('\n').slice(-n).join('\n');

async function one(p) {
  let where = path.join(dir, p.name);
  const step = (what, r) => ({ ok: false, step: what, detail: tail(r.out) });
  fs.mkdirSync(where, { recursive: true });
  let r = run('git', ['init', '-q'], where);
  // `dir`: a project that is a folder of a larger repository; only that folder is fetched.
  if (p.dir) {
    r = run('git', ['sparse-checkout', 'set', p.dir], where);
    if (!r.ok) return step('sparse-checkout', r);
  }
  r = run('git', ['fetch', '-q', '--depth', '1', ...(p.dir ? ['--filter=blob:none'] : []), p.repo, p.sha], where);
  if (!r.ok) return step('fetch', r);
  r = run('git', ['checkout', '-q', 'FETCH_HEAD'], where);
  if (!r.ok) return step('checkout', r);
  if (p.dir) where = path.join(where, p.dir);
  // `install: false` is for a project with no dependencies to build (the source is all there is).
  if (p.install !== false) {
    if (p.dropLockfile) fs.rmSync(path.join(where, 'pnpm-lock.yaml'), { force: true });
    const lock = fs.existsSync(path.join(where, 'pnpm-lock.yaml'));
    r = run('pnpm', ['install', '--ignore-scripts', lock ? '--frozen-lockfile' : '--no-frozen-lockfile', ...(p.installArgs || [])], where, 900);
    if (!r.ok) return step('install', r);
  }
  const started = Date.now();
  r = run(rkp, p.build, where, 300);
  const ms = Date.now() - started;
  if (!r.ok) return { ...step('build', r), ms };
  for (const f of p.expect?.files || []) if (!fs.existsSync(path.join(where, f))) return { ok: false, step: 'output', detail: `missing ${f}`, ms };
  // `run`: a program built for Node is run with arguments and must print what is expected.
  for (const check of p.expect?.run || []) {
    r = run(process.execPath, [path.join(where, p.expect.files[0]), ...check.args], where, 60);
    if (!r.ok) return { ...step('run', r), ms };
    if (!new RegExp(check.stdout, 'm').test(r.out)) return { ok: false, step: 'run', detail: `${check.args.join(' ')} printed ${JSON.stringify(r.out.slice(0, 200))}, expected /${check.stdout}/`, ms };
  }
  if (p.kind === 'app' && browser) {
    const outdir = p.build[p.build.indexOf('--outdir') + 1] || 'dist';
    const page = await checkPage(path.join(where, outdir), p.expect?.page, browser);
    if (!page.ok) return { ok: false, step: 'page', detail: `${page.problems.join('\n')}\nbody: ${page.html}${page.log.length ? '\nlog: ' + page.log.join('\n     ') : ''}`, ms };
  }
  if (p.kind === 'library' && p.expect?.exports) {
    const file = path.join(where, p.expect.files[0]);
    const probe = `import(${JSON.stringify(pathToFileURL(file).href)}).then((m) => console.log(JSON.stringify(Object.keys(m))), (e) => { console.error(e.stack || e); process.exit(1); })`;
    r = run(process.execPath, ['-e', probe], where, 60);
    if (!r.ok) return { ...step('import', r), ms };
    const have = JSON.parse(r.out.split('\n').pop());
    const lacking = p.expect.exports.filter((n) => !have.includes(n));
    if (lacking.length) return { ok: false, step: 'exports', detail: `missing ${lacking.join(', ')}; has ${have.join(', ')}`, ms };
    // `smoke`: an expression over the module `m` that must print the given text, so the output does what the library does.
    const smoke = p.expect.smoke;
    if (smoke) {
      const code = `import(${JSON.stringify(pathToFileURL(file).href)}).then((m) => console.log(String(${smoke.code})), (e) => { console.error(e.stack || e); process.exit(1); })`;
      r = run(process.execPath, ['-e', code], where, 60);
      if (!r.ok) return { ...step('smoke', r), ms };
      if (r.out.split('\n').pop() !== smoke.equals) return { ok: false, step: 'smoke', detail: `${smoke.code} gave ${r.out.split('\n').pop()}, expected ${smoke.equals}`, ms };
    }
  }
  return { ok: true, ms };
}

const results = [];
for (const p of list) {
  process.stdout.write(`${p.name.padEnd(28)}`);
  let r = { name: p.name, kind: p.kind, sha: p.sha, ...(await one(p)) };
  // A known gap is expected to fail; one that passes is news (the entry should lose its `expectFail`).
  if (p.expectFail) r = r.ok ? { ...r, flagged: true } : { ...r, ok: true, expected: true, step: r.step, detail: r.detail };
  results.push(r);
  const lines = (text) => (text || '').split('\n').map((l) => '    ' + l).join('\n');
  if (p.expectFail) console.log(r.expected ? `xfail ${r.step}: ${p.expectFail}\n${lines(r.detail)}` : `XPASS ${r.ms} ms: this passes now, so remove its expectFail (${p.expectFail})`);
  else console.log(r.ok ? `ok   ${r.ms} ms` : `FAIL ${r.step}\n${lines(r.detail)}`);
}
const passed = results.filter((r) => r.ok && !r.flagged).length;
console.log(`\n${browser ? '' : '(no browser: apps were built but not loaded)\n'}${passed} of ${results.length} built and checked out (${temporary ? 'work folder deleted' : `work folder ${dir}, left in place`})`);
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify({ rkp: spawnSync(rkp, ['--version'], { encoding: 'utf8' }).stdout.trim(), results }, null, 2) + '\n');
process.exit(passed === results.length ? 0 : 1);
