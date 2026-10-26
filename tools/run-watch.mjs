// Tests for --watch: the bundler rebuilds when a file it read changes, plugins hear about it (buildStart,
// watchChange, closeWatcher), a changed config starts everything again, and Ctrl-C ends it cleanly.
//   node tools/run-watch.mjs
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const dir = mkdtempSync(join(tmpdir(), 'watch-'));
const out = join(dir, 'out', 'bundle.mjs');
const log = join(dir, 'hooks.log');
const write = (name, text) => writeFileSync(join(dir, name), text);
const config = (banner) => `
import fs from 'node:fs';
const note = (text) => fs.appendFileSync(${JSON.stringify(log)}, text + '\\n');
export default {
  plugins: [{
    name: 'recorder',
    buildStart() { note('start watch=' + this.meta.watchMode); },
    watchChange(id, { event }) { note('change ' + id.split(/[\\\\/]/).pop() + ' ' + event); },
    renderChunk(code) { return { code: ${JSON.stringify(banner)} + '\\n' + code, map: null }; },
    closeWatcher() { note('closeWatcher'); },
  }],
};
`;
write('dep.js', 'export const value = "v1";\n');
write('main.js', 'import { value } from "./dep.js";\nconsole.log(value);\n');
write('rathkit.config.mjs', config('// config one'));
writeFileSync(log, '');

const child = spawn(bin, ['bundle', 'main.js', '--outfile', out, '--watch'], { cwd: dir, stdio: ['ignore', 'ignore', 'pipe'] });
let stderr = '';
child.stderr.on('data', (d) => (stderr += d));
const exited = new Promise((resolve) => child.on('exit', (code, signal) => resolve({ code, signal })));

const until = async (what, check, ms = 15000) => {
  const start = Date.now();
  while (Date.now() - start < ms) {
    try {
      if (check()) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 40));
  }
  throw new Error(`timed out waiting for ${what}\n--- stderr\n${stderr}\n--- log\n${readFileSync(log, 'utf8')}`);
};
const output = () => readFileSync(out, 'utf8');
let failed = null;
try {
  await until('the first build', () => output().includes('v1') && output().startsWith('// config one'));

  write('dep.js', 'export const value = "v2";\n');
  await until('a rebuild after dep.js changed', () => output().includes('v2'));

  // A file the build did not read before: main.js now imports it, and it is watched from then on.
  write('extra.js', 'export const extra = "x1";\n');
  write('main.js', 'import { value } from "./dep.js";\nimport { extra } from "./extra.js";\nconsole.log(value, extra);\n');
  await until('a rebuild with extra.js', () => output().includes('x1'));
  // A change of size too: the watcher compares the modification time and the size, and a write that follows the last build by less
  // than a clock tick of the file system (a few milliseconds) with the same size looks like no change.
  write('extra.js', 'export const extra = "x2-changed";\n');
  await until('a rebuild after extra.js changed', () => output().includes('x2-changed'));

  // A file added to a directory that `import.meta.glob` lists is picked up, though no file the build read changed.
  mkdirSync(join(dir, 'items'));
  write('items/a.js', 'export default "g1";\n');
  write('main.js', 'import { value } from "./dep.js";\nimport { extra } from "./extra.js";\nconst items = import.meta.glob("./items/*.js", { eager: true, import: "default" });\nconsole.log(value, extra, items);\n');
  await until('a build that lists items', () => output().includes('g1'));
  write('items/b.js', 'export default "g2";\n');
  await until('a rebuild with a file added to the glob', () => output().includes('g2'));

  // The config is read again, so the new plugin's banner shows.
  write('rathkit.config.mjs', config('// config two'));
  await until('a restart with the new config', () => output().startsWith('// config two'));

  write('dep.js', 'export const value = "v3";\n');
  await until('a rebuild after the restart', () => output().includes('v3'));

  child.kill('SIGINT');
  const { code } = await Promise.race([exited, new Promise((_, no) => setTimeout(() => no(new Error('did not stop on SIGINT')), 10000))]);
  // Windows has no SIGINT to send a child: Node ends it at once, so the clean stop (exit 0, closeWatcher) can be checked on other systems only.
  const windows = process.platform === 'win32';
  if (!windows && code !== 0) throw new Error(`exit code ${code}\n${stderr}`);
  const lines = readFileSync(log, 'utf8').trim().split('\n');
  const text = lines.join(' | ');
  for (const wanted of ['start watch=true', 'change dep.js update', 'change extra.js update', ...(windows ? [] : ['closeWatcher'])]) {
    if (!text.includes(wanted)) throw new Error(`the plugin never logged "${wanted}": ${text}`);
  }
  // Every change comes before the build it causes.
  const first = lines.indexOf('change dep.js update');
  if (lines[first + 1] !== 'start watch=true') throw new Error(`expected a build to follow the change: ${text}`);
} catch (error) {
  failed = error;
  child.kill('SIGKILL');
}
if (failed) {
  console.log(`FAIL watch: ${failed.message}`);
  process.exit(1);
}
console.log('watch: rebuilds, hooks, restart on config change and Ctrl-C all work');
// A killed server (and its plugin host) may still hold the folder for a moment on Windows, which cannot delete what a process is in.
try {
  rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
} catch (error) {
  if (process.platform !== 'win32') throw error;
}
