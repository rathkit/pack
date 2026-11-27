// What rkp writes, changes and deletes (the section "What Pack writes and deletes" of the README is this table). Each command runs in a project of
// its own, with HOME, the cache folder and the temporary folder inside the sandbox, and the whole sandbox is compared before and after: any file
// that appears, changes or goes and is not in the expected list fails the run, so a new write has to be stated here (and in the README).
//   node tools/run-writes.mjs
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import https from 'node:https';
import http from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', process.platform === 'win32' ? 'rkp.exe' : 'rkp');
if (process.platform === 'win32') {
  console.log('writes: skipped on Windows (the cache folder differs and the sandbox uses symbolic links)');
  process.exit(0);
}
if (!existsSync(bin)) {
  console.error(`rkp not found at ${bin}; run cargo build --release or set RATHKIT_PACK_BIN`);
  process.exit(2);
}
// TypeScript, for --dts: the one the other suites use.
const ts = [process.env.TS_MODULE, join(root, 'test-deps', 'node_modules', 'ts5')].find((p) => p && existsSync(join(p, 'package.json')));
const sandboxes = mkdtempSync(join(tmpdir(), 'writes-'));

const put = (file, text) => {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, text);
};
const project = (sb) => {
  const p = join(sb, 'proj');
  put(join(p, 'package.json'), '{ "name": "demo", "version": "1.0.0", "type": "module" }\n');
  put(join(p, 'tsconfig.json'), '{ "compilerOptions": { "target": "es2022", "module": "esnext", "strict": true } }\n');
  put(join(p, 'src/greet.ts'), '/*! legal: Demo 1.0 */\nexport function greet(name: string): string { return `Hello, ${name}!`; }\n');
  put(join(p, 'src/main.ts'), "import { greet } from './greet';\nconsole.log(greet('x'));\n");
  put(join(p, 'src/big.js'), Array.from({ length: 700 }, (_, i) => `export function f${i}(a) { const doubled = a * 2; return doubled + ${i}; }\n`).join('') + 'console.log(f1(1));\n');
  put(join(p, 'index.html'), '<!doctype html><html><head><link rel="stylesheet" href="./style.css"></head><body><script type="module" src="./page.js"></script></body></html>\n');
  put(join(p, 'page.js'), "document.title = 'x';\n");
  put(join(p, 'style.css'), 'h1 { color: red; }\n');
  put(join(p, 'public/robots.txt'), 'User-agent: *\n');
  put(join(p, 'dist/old.txt'), 'a file that was there before the build\n');
  put(join(p, 'vite.config.js'), "export default { build: { outDir: 'dist' } };\n");
  for (const d of ['home/.cache', 'tmp']) mkdirSync(join(sb, d), { recursive: true });
  if (ts) symlinkSync(ts.replace(/\/ts5$/, ''), join(p, 'node_modules'), 'dir');
  return p;
};
const snapshot = (sb) => {
  const out = new Map();
  // The entries of a folder say what they are (a link is skipped), so no file is looked at and then read.
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      const rel = relative(sb, full);
      if (rel === 'proj/node_modules') continue;
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile()) out.set(rel, createHash('sha1').update(readFileSync(full)).digest('hex'));
    }
  };
  walk(sb);
  return out;
};
const sorted = (a) => [...a].sort();

// `created`, `modified`, `deleted`: the sandbox's files by relative name; a `*` matches any run of characters in one name.
const cases = [
  { name: '--outfile', args: ['bundle', 'src/main.ts', '--outfile', 'out/app.mjs'], created: ['proj/out/app.mjs'] },
  { name: '--outdir leaves what is in the folder', args: ['bundle', 'src/main.ts', '--outdir', 'dist'], created: ['proj/dist/bundle.js'] },
  { name: '--sourcemap', args: ['bundle', 'src/main.ts', '--outfile', 'out/app.mjs', '--sourcemap'], created: ['proj/out/app.mjs', 'proj/out/app.mjs.map'] },
  { name: '--sourcemap=inline', args: ['bundle', 'src/main.ts', '--outfile', 'out/app.mjs', '--sourcemap=inline'], created: ['proj/out/app.mjs'] },
  { name: '--metafile', args: ['bundle', 'src/main.ts', '--outdir', 'dist', '--metafile=meta.json'], created: ['proj/dist/bundle.js', 'proj/meta.json'] },
  { name: '--depfile', args: ['bundle', 'src/main.ts', '--outdir', 'dist', '--depfile=deps.json'], created: ['proj/dist/bundle.js', 'proj/deps.json'] },
  { name: '--legal-comments=linked', args: ['bundle', 'src/main.ts', '--outfile', 'out/app.mjs', '--legal-comments=linked'], created: ['proj/out/app.mjs', 'proj/out/app.mjs.LEGAL.txt'] },
  { name: '--legal-comments=external', args: ['bundle', 'src/main.ts', '--outfile', 'out/app.mjs', '--legal-comments=external'], created: ['proj/out/app.mjs', 'proj/out/app.mjs.LEGAL.txt'] },
  ...(ts
    ? [
        { name: '--dts', args: ['bundle', 'src/main.ts', '--outdir', 'dist', '--dts'], created: ['proj/dist/bundle.js', 'proj/dist/greet.d.ts', 'proj/dist/main.d.ts'] },
        { name: '--dts-dir', args: ['bundle', 'src/main.ts', '--outdir', 'dist', '--dts-dir=types'], created: ['proj/dist/bundle.js', 'proj/types/greet.d.ts', 'proj/types/main.d.ts'] },
      ]
    : []),
  { name: '--write-exports edits package.json', args: ['bundle', 'src/main.ts', '--outdir', 'dist', '--splitting', '--write-exports'], created: ['proj/dist/main.js'], modified: ['proj/package.json'] },
  { name: 'a page and public/', args: ['bundle', 'index.html', '--outdir', 'dist'], created: ['proj/dist/index.html', 'proj/dist/page.js', 'proj/dist/robots.txt', 'proj/dist/style.css'] },
  { name: '--metafile lists a page, its files and the maps', args: ['bundle', 'index.html', '--outdir', 'dist', '--sourcemap', '--metafile=meta.json'], created: ['proj/dist/index.html', 'proj/dist/page.js', 'proj/dist/page.js.map', 'proj/dist/robots.txt', 'proj/dist/style.css', 'proj/dist/style.css.map', 'proj/meta.json'] },
  { name: 'a build over its own input stops', args: ['bundle', 'src/main.ts', '--outfile', 'src/main.ts'], created: [], expectFail: true },
  { name: 'rkp migrate prints', args: ['migrate'], created: [] },
  { name: 'rkp migrate --write edits package.json', args: ['migrate', '--write'], modified: ['proj/package.json'] },
  { name: 'rkp analyze prints', args: ['analyze', 'meta.json'], setup: (p) => spawnSync(bin, ['bundle', 'src/main.ts', '--outdir', 'dist0', '--metafile=meta.json'], { cwd: p }), created: [], ignoreSetup: true },
  { name: 'RATHKIT_CACHE=off writes no cache entry', args: ['bundle', 'src/big.js', '--minify', '--outfile', 'out/app.mjs'], env: { RATHKIT_CACHE: 'off' }, created: ['proj/out/app.mjs'] },
  { name: 'the minifier cache is on by default and writes its entries in the cache folder', args: ['bundle', 'src/big.js', '--minify', '--outfile', 'out/app.mjs'], created: ['proj/out/app.mjs', 'home/.cache/rathkit/minify/*.min'] },
  { name: 'rkp serve writes nothing', args: ['serve', 'index.html'], serve: 5391, created: [] },
  { name: 'rkp serve --ready-file writes that file', args: ['serve', 'index.html', '--ready-file=ready.json'], serve: 5393, created: ['proj/ready.json'] },
  { name: 'rkp serve --https keeps a certificate and key in the cache folder', args: ['serve', 'index.html', '--https'], serve: 5392, created: ['home/.cache/rathkit/dev-cert.pem', 'home/.cache/rathkit/dev-key.pem'] },
];

const matches = (pattern, name) => new RegExp('^' + pattern.split('*').map((s) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]*') + '$').test(name);
// `ca`: for https, the certificate the server made for itself (it is the only one the request trusts: validation stays on).
const request = (url, ca) =>
  new Promise((resolve) => {
    const secure = url.startsWith('https');
    if (secure && !ca) return resolve(0);
    const lib = secure ? https : http;
    const req = lib.get(url, { ...(secure ? { ca } : {}), timeout: 2000 }, (res) => {
      res.resume();
      resolve(res.statusCode);
    });
    req.on('error', () => resolve(0));
    req.on('timeout', () => req.destroy());
  });

let failures = 0;
let listed = 0;
for (const c of cases) {
  const sb = mkdtempSync(join(sandboxes, 'case-'));
  const p = project(sb);
  c.setup?.(p);
  const before = snapshot(sb);
  const env = { ...process.env, HOME: join(sb, 'home'), XDG_CACHE_HOME: join(sb, 'home/.cache'), TMPDIR: join(sb, 'tmp'), TEMP: join(sb, 'tmp'), ...(c.env ?? {}) };
  if (!c.env?.RATHKIT_CACHE) delete env.RATHKIT_CACHE;
  delete env.RATHKIT_CACHE_DIR;
  let problem = null;
  if (c.serve) {
    const child = spawn(bin, [...c.args, '--port', String(c.serve)], { cwd: p, env, stdio: 'ignore' });
    let status = 0;
    for (let i = 0; i < 40 && status !== 200; i++) {
      await new Promise((r) => setTimeout(r, 250));
      let cert;
      try {
        cert = readFileSync(join(sb, 'home/.cache/rathkit/dev-cert.pem'));
      } catch {
        // the server has not made it yet
      }
      status = await request(`${c.args.includes('--https') ? 'https' : 'http'}://localhost:${c.serve}/`, cert);
    }
    await new Promise((r) => setTimeout(r, 300));
    child.kill('SIGINT');
    await new Promise((r) => setTimeout(r, 300));
    if (status !== 200) problem = `the server answered ${status}`;
  } else {
    // A build also says what it wrote: `--list-files` (the Node API's `files`) and the metafile are compared with the folder below.
    const lists = c.args[0] === 'bundle' && !c.expectFail;
    const listFile = join(sandboxes, `list-${listed++}.txt`);
    const r = spawnSync(bin, lists ? [...c.args, `--list-files=${listFile}`] : c.args, { cwd: p, env, encoding: 'utf8', timeout: 120000 });
    c.listFile = lists ? listFile : null;
    if (c.expectFail ? r.status === 0 : r.status !== 0) problem = c.expectFail ? 'it should have stopped with an error' : `exit ${r.status}: ${(r.stderr || '').split('\n')[0]}`;
  }
  const after = snapshot(sb);
  const createdNow = sorted([...after.keys()].filter((k) => !before.has(k)));
  const deletedNow = sorted([...before.keys()].filter((k) => !after.has(k)));
  const modifiedNow = sorted([...after.keys()].filter((k) => before.has(k) && before.get(k) !== after.get(k)));
  const check = (label, got, want = []) => {
    const left = got.filter((g) => !want.some((w) => matches(w, g)));
    const missing = want.filter((w) => !got.some((g) => matches(w, g)));
    if (left.length) problem ??= `${label} something that is not stated: ${left.join(', ')}`;
    if (missing.length) problem ??= `${label} less than stated: ${missing.join(', ')}`;
  };
  if (!problem && c.listFile) {
    // What the program lists is what is on disk: the files that appeared and the files it changed (`--write-exports`), none missing, none extra.
    const written = readFileSync(c.listFile, 'utf8').split('\n').filter(Boolean).map((f) => relative(sb, f));
    const onDisk = [...createdNow, ...modifiedNow].filter((f) => f.startsWith('proj/'));
    if (JSON.stringify(sorted(written)) !== JSON.stringify(sorted(onDisk))) problem = `--list-files says ${sorted(written).join(', ')}; the folder has ${sorted(onDisk).join(', ')}`;
    // The metafile lists the same files with a kind, but not itself.
    const meta = c.args.find((a) => a.startsWith('--metafile='))?.slice('--metafile='.length);
    if (!problem && meta) {
      const outputs = JSON.parse(readFileSync(join(p, meta), 'utf8')).outputs;
      const names = sorted(Object.keys(outputs).map((k) => relative(sb, join(p, k))));
      const want = sorted(onDisk.filter((f) => f !== join('proj', meta)));
      if (JSON.stringify(names) !== JSON.stringify(want)) problem = `the metafile's outputs are ${names.join(', ')}; the folder has ${want.join(', ')}`;
      else if (Object.values(outputs).some((o) => typeof o.kind !== 'string')) problem = 'an output of the metafile has no kind';
    }
  }
  if (!problem) {
    check('created', createdNow, c.created);
    check('modified', modifiedNow, c.modified);
    check('deleted', deletedNow, c.deleted);
  }
  if (problem) {
    failures++;
    console.log(`FAIL ${c.name}: ${problem}`);
  } else console.log(`ok   ${c.name}`);
}
rmSync(sandboxes, { recursive: true, force: true });
if (failures) {
  console.log(`${failures} command(s) wrote, changed or deleted something that is not stated`);
  process.exit(1);
}
console.log(`all ${cases.length} commands write, change and delete only what the README states`);
