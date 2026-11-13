// Tests for `rkp serve --hmr`: the served build is a table of modules; changing a file sends an update, and the table replaces the modules
// in place (here in Node, with the page client's job done by this script): accepted by the module itself, by its importer, and not
// accepted at all (the page would reload).
//   node tools/run-hmr.mjs
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, renameSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';
import { browserKind, findBrowser, openBrowser } from './lib/browsers.mjs';
import { existsSync, cpSync, symlinkSync } from 'node:fs';
import { testModules } from './lib/test-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const dir = mkdtempSync(join(tmpdir(), 'hmr-'));
const write = (name, text) => {
  mkdirSync(dirname(join(dir, name)), { recursive: true });
  writeFileSync(join(dir, name), text);
};
write('index.html', '<!doctype html><html><body><script type="module" src="/main.js"></script></body></html>\n');
write('main.js', `
import { value } from './counter.js';
import { label } from './label.js';
import def, { helper } from './lib.cjs';
globalThis.log = globalThis.log || [];
import('./waits.js').then((m) => globalThis.log.push('dynamic wait ' + m.w));
globalThis.log.push('main ran ' + value + ' ' + label() + ' ' + def.kind + ' ' + helper());
if (import.meta.hot) {
  import.meta.hot.accept('./counter.js', (mod) => globalThis.log.push('main saw counter ' + mod.value + ' ' + value));
}
`);
write('counter.js', 'export const value = 1;\n');
write('waits.js', "export const w = await Promise.resolve(7);\nif (import.meta.hot) import.meta.hot.accept((m) => globalThis.log.push('waits accepted ' + m.w));\n");
write('label.js', `
export let count = (import.meta.hot?.data.count ?? 0) + 1;
export const label = () => 'label' + count;
if (import.meta.hot) {
  import.meta.hot.dispose((data) => { data.count = count; globalThis.log.push('label disposed'); });
  import.meta.hot.accept((mod) => globalThis.log.push('label accepted ' + mod.label()));
}
`);
write('lib.cjs', 'exports.kind = "cjs"; exports.helper = () => "h"; module.exports.default = undefined; module.exports = Object.assign(module.exports, { kind: "cjs" });\n');

const child = spawn(bin, ['serve', 'main.js', '--hmr', '--port', '0'], { cwd: dir, stdio: ['ignore', 'pipe', 'pipe'] });
let output = '';
child.stdout.on('data', (d) => (output += d));
child.stderr.on('data', (d) => (output += d));
let failed = 0;
let checks = 0;
const check = (ok, what, detail = '') => {
  checks++;
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n  ' + detail : ''}`);
  }
};
const until = async (condition, ms = 15000) => {
  const started = Date.now();
  while (Date.now() - started < ms) {
    if (await condition()) return true;
    await new Promise((r) => setTimeout(r, 50));
  }
  return false;
};
const until60 = (condition) => until(condition, 60000);
// The server's first build, which the page asks for as soon as it can. A build that is not served in 20 seconds is a failure that
// says what the server printed and what the last request got.
const fetchBuilt = async (base, log = () => output) => {
  let response;
  let last = 'no answer';
  await until(async () => {
    response = await fetch(base + '/bundle.js').catch((error) => {
      last = String(error?.cause?.message ?? error?.message ?? error);
      return null;
    });
    if (response) last = `status ${response.status}`;
    return response?.ok;
  }, 20000);
  if (!response?.ok) throw new Error(`the build was not served from ${base} (${last}); the server said:\n${log()}`);
  return response.text();
};
try {
  if (!(await until(() => /localhost:\d+|127\.0\.0\.1:\d+/.test(output)))) throw new Error('the server did not start: ' + output);
  const port = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(output)[1];
  const base = `http://127.0.0.1:${port}`;
  const built = await fetchBuilt(base);
  check(/__r\.def\(/.test(built) && !/__toESM|var __cjs/.test(built), 'the build is a table of modules', built.slice(0, 300));

  // The messages the page would get.
  const messages = [];
  const controller = new AbortController();
  fetch(base + '/__rathkit/events', { signal: controller.signal }).then(async (response) => {
    const reader = response.body.getReader();
    let buffer = '';
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      buffer += Buffer.from(value).toString();
      let at;
      while ((at = buffer.indexOf('\n\n')) >= 0) {
        const line = buffer.slice(0, at).replace(/^data: /, '');
        buffer = buffer.slice(at + 2);
        try {
          messages.push(JSON.parse(line));
        } catch {}
      }
    }
  }).catch(() => {});

  // The page: the build run in this process.
  globalThis.location = { reload() { globalThis.reloaded = (globalThis.reloaded ?? 0) + 1; } };
  const file = join(dir, 'page.mjs');
  writeFileSync(file, built);
  await import(pathToFileURL(file).href);
  check(globalThis.log[0] === 'main ran 1 label1 cjs h', 'the modules ran', JSON.stringify(globalThis.log));
  check(await until(() => globalThis.log.includes('dynamic wait 7')), 'a dynamic import of a module that waits is waited for', JSON.stringify(globalThis.log));

  const next = async (type) => {
    const before = messages.length;
    const found = await until(() => messages.slice(before).some((m) => m.type === type));
    return found ? messages.slice(before).find((m) => m.type === type) : undefined;
  };

  // 1. counter.js changes; main.js accepts it: main does not run again, its callback gets the new module.
  const wait = next('hmr');
  write('counter.js', 'export const value = 2;\n');
  const first = await wait;
  check(first && Object.keys(first.modules).length === 1 && Object.keys(first.modules)[0].endsWith('counter.js'), 'only the changed module is sent', JSON.stringify(first && Object.keys(first.modules)));
  check((await globalThis.__rathkit_hmr.update(first)) === true, 'the update is applied');
  check(globalThis.log.at(-1) === 'main saw counter 2 2' && globalThis.log.filter((l) => l.startsWith('main ran')).length === 1, 'the accepting importer is called, and does not run again', JSON.stringify(globalThis.log));

  // 2. label.js accepts itself: it is disposed, runs again with its data, and its callback gets the new module.
  const wait2 = next('hmr');
  write('label.js', readFileSync(join(dir, 'label.js'), 'utf8').replace("'label'", "'LABEL'"));
  const second = await wait2;
  check((await globalThis.__rathkit_hmr.update(second)) === true, 'a self-accepting module is replaced');
  check(globalThis.log.includes('label disposed') && globalThis.log.at(-1) === 'label accepted LABEL2', 'dispose ran, data carried over, the callback got the new module', JSON.stringify(globalThis.log.slice(-3)));

  // A module with top-level await is replaced too: the callback runs when the new one has finished.
  const waitAsync = next('hmr');
  write('waits.js', readFileSync(join(dir, 'waits.js'), 'utf8').replace('Promise.resolve(7)', 'new Promise((r) => setTimeout(() => r(8), 20))'));
  const asyncUpdate = await waitAsync;
  const applying = globalThis.__rathkit_hmr.update(asyncUpdate);
  check(applying instanceof Promise && globalThis.log.at(-1) !== 'waits accepted 8', 'an update of a module that waits answers with a promise', String(applying));
  check((await applying) === true && globalThis.log.at(-1) === 'waits accepted 8', 'and applies it when the module has run', JSON.stringify(globalThis.log.slice(-2)));

  // A module that gains an import of a new file: both are sent, and the new one runs.
  const wait2b = next('hmr');
  write('extra.js', "export const extra = 'extra!';\n");
  write('label.js', readFileSync(join(dir, 'label.js'), 'utf8').replace("export const label", "import { extra } from './extra.js';\nexport const extraLabel = () => extra;\nexport const label"));
  const added = await wait2b;
  check(added && Object.keys(added.modules).some((k) => k.endsWith('extra.js')) && Object.keys(added.modules).some((k) => k.endsWith('label.js')), 'a new module is sent with the one that imports it', JSON.stringify(added && Object.keys(added.modules)));
  check((await globalThis.__rathkit_hmr.update(added)) === true && globalThis.log.at(-1).startsWith('label accepted'), 'and the update is applied');
  // And one that loses it: the file is reported as removed.
  const wait2c = next('hmr');
  write('label.js', readFileSync(join(dir, 'label.js'), 'utf8').replace("import { extra } from './extra.js';\nexport const extraLabel = () => extra;\n", ''));
  rmSync(join(dir, 'extra.js'));
  const dropped = await wait2c;
  check(dropped && dropped.removed.some((k) => k.endsWith('extra.js')), 'a module that is gone is reported', JSON.stringify(dropped));
  check((await globalThis.__rathkit_hmr.update(dropped)) === true, 'and the update is applied without it', JSON.stringify({ dropped, log: globalThis.log.slice(-4) }));

  // 3. lib.cjs is imported by main.js, which accepts nothing about it: the change reaches an entry, so the page reloads.
  const wait3 = next('hmr');
  write('lib.cjs', readFileSync(join(dir, 'lib.cjs'), 'utf8').replace('"h"', '"H"'));
  const third = await wait3;
  check(third && (await globalThis.__rathkit_hmr.update(third)) === false, 'a change nothing accepts asks for a reload');

  controller.abort();
} catch (error) {
  failed++;
  console.log('FAIL ' + (error?.stack ?? error));
} finally {
  child.kill();
}

// The same program, run as a normal bundle and as the table: it must print the same.
{
  const app = join(root, 'tests', 'hmr', 'app');
  const run = (file) => spawnSync('node', [file], { encoding: 'utf8' });
  const normal = join(dir, 'normal.mjs');
  const built = spawnSync(bin, ['bundle', 'main.js', '--outfile', normal, '--format', 'esm'], { cwd: app, encoding: 'utf8' });
  check(built.status === 0, 'the normal bundle builds', built.stderr);
  const want = run(normal);
  const served = spawn(bin, ['serve', 'main.js', '--hmr', '--port', '0'], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  served.stdout.on('data', (d) => (log += d));
  served.stderr.on('data', (d) => (log += d));
  try {
    if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(log)))) throw new Error('the server did not start: ' + log);
    const port = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(log)[1];
    const table = await fetchBuilt('http://127.0.0.1:' + port, () => log);
    const file = join(dir, 'table.mjs');
    writeFileSync(file, table);
    const got = run(file);
    check(want.status === 0, 'the normal bundle runs', want.stderr);
    check(got.status === 0, 'the table runs', got.stderr);
    check(got.stdout === want.stdout, 'the table prints what the bundle prints', '--- bundle\n' + want.stdout + '--- table\n' + got.stdout);
  } catch (error) {
    failed++;
    console.log('FAIL ' + (error?.stack ?? error));
  } finally {
    served.kill();
  }
}

// Decorators, enums, namespaces and top-level await (in the entry, in a dependency, through a re-export, and in a dynamic import).
{
  const app = join(root, 'tests', 'hmr', 'decorators-tla');
  const normal = join(dir, 'dt-normal.mjs');
  const built = spawnSync(bin, ['bundle', 'main.ts', '--outfile', normal, '--format', 'esm'], { cwd: app, encoding: 'utf8' });
  check(built.status === 0, 'the decorators program builds as a bundle', built.stderr);
  const want = spawnSync('node', [normal], { encoding: 'utf8' });
  const served = spawn(bin, ['serve', 'main.ts', '--hmr', '--port', '0'], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  served.stdout.on('data', (d) => (log += d));
  served.stderr.on('data', (d) => (log += d));
  try {
    if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(log)))) throw new Error('the server did not start: ' + log);
    const port = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(log)[1];
    const table = await fetchBuilt('http://127.0.0.1:' + port, () => log);
    check(!/does not support/.test(log), 'decorators and top-level await are accepted', log);
    const file = join(dir, 'dt-table.mjs');
    writeFileSync(file, table);
    const got = spawnSync('node', [file], { encoding: 'utf8' });
    check(want.status === 0 && want.stdout.includes('main-waited'), 'the program runs as a bundle', want.stderr + want.stdout);
    check(got.status === 0, 'and as a table', got.stderr);
    check(got.stdout === want.stdout, 'the table prints the same, the awaits in the same order', '--- bundle\n' + want.stdout + '--- table\n' + got.stdout + got.stderr);
  } catch (error) {
    failed++;
    console.log('FAIL ' + (error?.stack ?? error));
  } finally {
    served.kill();
  }
}

// React: JSX, named imports from CommonJS packages, CSS modules.
if (existsSync(join(testModules, 'react-dom'))) {
  const app = join(dir, 'react-app');
  cpSync(join(root, 'tests', 'hmr', 'react'), app, { recursive: true });
  symlinkSync(testModules, join(app, 'node_modules'), 'dir');
  const define = ['--define', 'process.env.NODE_ENV="development"'];
  const normal = join(dir, 'react-normal.mjs');
  const built = spawnSync(bin, ['bundle', 'main.jsx', '--outfile', normal, '--format', 'esm', ...define], { cwd: app, encoding: 'utf8' });
  check(built.status === 0, 'the React bundle builds', built.stderr);
  const want = spawnSync('node', [normal], { encoding: 'utf8' });
  const served = spawn(bin, ['serve', 'main.jsx', '--hmr', '--port', '0', ...define], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  served.stdout.on('data', (d) => (log += d));
  served.stderr.on('data', (d) => (log += d));
  try {
    if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(log)))) throw new Error('the server did not start: ' + log);
    const port = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(log)[1];
    const table = await fetchBuilt('http://127.0.0.1:' + port, () => log);
    const file = join(app, 'table.mjs');
    writeFileSync(file, table);
    const got = spawnSync('node', [file], { encoding: 'utf8', cwd: app });
    check(want.status === 0 && want.stdout.includes('hello <!-- -->table'), 'the React bundle runs', want.stderr + want.stdout);
    check(got.status === 0, 'the React table runs', got.stderr);
    check(got.stdout === want.stdout, 'React prints the same from the table', '--- bundle\n' + want.stdout + '--- table\n' + got.stdout + got.stderr);
  } catch (error) {
    failed++;
    console.log('FAIL ' + (error?.stack ?? error));
  } finally {
    served.kill();
  }
}

// Large real packages: the table prints what a normal bundle prints (CommonJS interop, cycles, a thousand modules).
for (const name of ['rxjs', 'lodash', 'three']) {
  if (!existsSync(join(testModules, name === 'lodash' ? 'lodash-es' : name))) continue;
  const app = join(dir, `soak-${name}`);
  cpSync(join(root, 'tests', 'hmr', 'soak', name), app, { recursive: true });
  symlinkSync(testModules, join(app, 'node_modules'), 'dir');
  writeFileSync(join(app, 'index.html'), '<!doctype html><script type="module" src="/main.js"></script>\n');
  const define = ['--define', 'process.env.NODE_ENV="development"'];
  const normal = join(dir, `soak-${name}-normal.mjs`);
  const built = spawnSync(bin, ['bundle', 'main.js', '--outfile', normal, '--format', 'esm', ...define], { cwd: app, encoding: 'utf8' });
  check(built.status === 0, `the ${name} bundle builds`, built.stderr);
  const want = spawnSync('node', [normal], { encoding: 'utf8', timeout: 60000 });
  const served = spawn(bin, ['serve', 'main.js', '--hmr', '--port', '0', ...define], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  served.stdout.on('data', (d) => (log += d));
  served.stderr.on('data', (d) => (log += d));
  try {
    if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(log)))) throw new Error('the server did not start: ' + log);
    const port = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(log)[1];
    const table = await fetchBuilt('http://127.0.0.1:' + port, () => log);
    const file = join(app, 'table.mjs');
    writeFileSync(file, table);
    const got = spawnSync('node', [file], { encoding: 'utf8', cwd: app, timeout: 60000 });
    check(want.status === 0 && want.stdout.length > 100, `the ${name} bundle runs`, want.stderr + want.stdout.slice(0, 200));
    check(got.status === 0, `the ${name} table runs`, got.stderr.slice(0, 600));
    check(got.stdout === want.stdout, `${name}: the table prints what the bundle prints`, '--- bundle\n' + want.stdout + '--- table\n' + got.stdout + got.stderr.slice(0, 300));
  } catch (error) {
    failed++;
    console.log('FAIL ' + (error?.stack ?? error));
  } finally {
    served.kill();
  }
}

// In a browser: the page client applies the updates, and the page is not reloaded.
{
  const kind = browserKind();
  const browser = findBrowser(kind);
  if (!browser) {
    console.log(`hmr: no ${kind} browser found (see tools/lib/browsers.mjs); the browser checks are skipped`);
  } else {
    const site = join(dir, 'site');
    const put = (name, text) => {
      mkdirSync(dirname(join(site, name)), { recursive: true });
      writeFileSync(join(site, name), text);
    };
    put('index.html', '<!doctype html><html><head><script>window.__page = Math.random();</script></head><body><div id="out"></div><script type="module" src="/main.js"></script></body></html>\n');
    put('main.js', "import './style.css';\nimport { text } from './view.js';\nimport { stamp } from './entry-only.js';\nconst show = () => (document.getElementById('out').textContent = text() + '|' + stamp);\nshow();\nif (import.meta.hot) import.meta.hot.accept('./view.js', () => show());\n");
    put('view.js', "export const text = () => 'one';\n");
    put('entry-only.js', "export const stamp = 'a';\n");
    put('style.css', 'body { color: rgb(1, 2, 3); }\n');
    const server = spawn(bin, ['serve', 'main.js', '--hmr', '--port', '0'], { cwd: site, stdio: ['ignore', 'pipe', 'pipe'] });
    let log = '';
    server.stdout.on('data', (d) => (log += d));
    server.stderr.on('data', (d) => (log += d));
    let tab;
    try {
      if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(log)))) throw new Error('the server did not start: ' + log);
      const port = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(log)[1];
      tab = await openBrowser(kind, browser);
      const pageEvents = []; // what the page printed and when it loaded, for the failure messages that explain a reload
      tab.onEvent((e) => {
        if (e.type === 'load' || e.type === 'exception' || (e.type === 'console' && /\[rkp\]/.test(e.text))) pageEvents.push(e.type === 'load' ? 'page loaded' : e.type === 'exception' ? 'exception: ' + e.text : e.text);
      });
      const evaluate = (expression) => tab.evaluate(expression);
      await tab.navigate(`http://127.0.0.1:${port}/`);
      check(await until(async () => (await evaluate("document.getElementById('out')?.textContent")) === 'one|a'), 'the page runs the modules');
      const page = await evaluate('window.__page');

      write2(site, 'view.js', "export const text = () => 'two';\n");
      check(await until(async () => (await evaluate("document.getElementById('out')?.textContent")) === 'two|a'), 'a module its importer accepts is replaced in the page');
      check((await evaluate('window.__page')) === page, 'and the page was not reloaded');

      write2(site, 'style.css', 'body { color: rgb(4, 5, 6); }\n');
      check(await until(async () => (await evaluate("getComputedStyle(document.body).color")) === 'rgb(4, 5, 6)'), 'a style sheet is swapped');
      check((await evaluate('window.__page')) === page, 'without a reload');

      write2(site, 'entry-only.js', "export const stamp = 'b';\n");
      check(await until(async () => (await evaluate("document.getElementById('out')?.textContent")) === 'two|b'), 'a change nothing accepts reloads the page');
      check((await evaluate('window.__page')) !== page, 'it is a new page');
      // React Fast Refresh: the state of a component survives its module being replaced.
      if (existsSync(join(testModules, 'react-refresh'))) {
        const app = join(dir, 'refresh-app');
        cpSync(join(root, 'tests', 'hmr', 'refresh'), app, { recursive: true });
        symlinkSync(testModules, join(app, 'node_modules'), 'dir');
        const react = spawn(bin, ['serve', 'main.jsx', '--hmr', '--port', '0', '--define', 'process.env.NODE_ENV="development"'], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
        let reactLog = '';
        react.stdout.on('data', (d) => (reactLog += d));
        react.stderr.on('data', (d) => (reactLog += d));
        const explainReload = explainReloadOf(pageEvents);
        try {
          if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(reactLog)))) throw new Error('the React server did not start: ' + reactLog);
          const reactPort = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(reactLog)[1];
          // The page is opened once the build is there to be served.
          await fetchBuilt(`http://127.0.0.1:${reactPort}`, () => reactLog);
          await tab.navigate(`http://127.0.0.1:${reactPort}/`);
          const text = () => evaluate("document.getElementById('out')?.textContent");
          check(await until60(async () => (await text()) === 'Hello count 0'), 'the React app renders', String(await text()));
          const first = await evaluate('window.__page');
          await evaluate("document.querySelector('button').click()");
          await evaluate("document.querySelector('button').click()");
          check(await until60(async () => (await text()) === 'Hello count 2'), 'and its state changes');
          write2(app, 'App.jsx', readFileSync(join(app, 'App.jsx'), 'utf8').replace('Hello', 'Howdy'));
          check(await until60(async () => (await text()) === 'Howdy count 2'), 'an edited component is replaced with its state kept', String(await text()));
          check((await evaluate('window.__page')) === first, 'without a reload');
          const hooksEdit = { log: reactLog.length, events: pageEvents.length };
          write2(app, 'App.jsx', readFileSync(join(app, 'App.jsx'), 'utf8').replace('useState(0)', 'useState(10)'));
          check(await until60(async () => (await text()) === 'Howdy count 10'), 'when its hooks change the state starts again', String(await text()));
          check((await evaluate('window.__page')) === first, 'and still no reload', explainReload(reactLog, hooksEdit));
          write2(app, 'App.jsx', readFileSync(join(app, 'App.jsx'), 'utf8') + 'export const notAComponent = 1;\n');
          check(await until60(async () => (await evaluate('window.__page')) !== first), 'a module that exports more than components reloads the page');
        } finally {
          react.kill();
        }
      }
      // Vue: single-file components through @vitejs/plugin-vue, whose generated code uses import.meta.hot.accept and Vue's HMR runtime.
      if (existsSync(join(testModules, '@vitejs', 'plugin-vue')) && existsSync(join(testModules, 'vue'))) {
        const app = join(dir, 'vue-app');
        cpSync(join(root, 'tests', 'hmr', 'vue'), app, { recursive: true });
        symlinkSync(testModules, join(app, 'node_modules'), 'dir');
        const defines = ['process.env.NODE_ENV="development"', '__VUE_OPTIONS_API__=true', '__VUE_PROD_DEVTOOLS__=false', '__VUE_PROD_HYDRATION_MISMATCH_DETAILS__=false'].flatMap((d) => ['--define', d]);
        const vue = spawn(bin, ['serve', 'main.js', '--hmr', '--port', '0', ...defines], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
        let vueLog = '';
        vue.stdout.on('data', (d) => (vueLog += d));
        vue.stderr.on('data', (d) => (vueLog += d));
        try {
          if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(vueLog)))) throw new Error('the Vue server did not start: ' + vueLog);
          const vuePort = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(vueLog)[1];
          await fetchBuilt(`http://127.0.0.1:${vuePort}`, () => vueLog);
          await tab.navigate(`http://127.0.0.1:${vuePort}/`);
          const text = () => evaluate("document.getElementById('app')?.textContent?.replace(/\\s+/g, ' ').trim()");
          check(await until60(async () => (await text()) === 'count 0Hellochild one'), 'the Vue app renders', String(await text()) + ' ' + vueLog.slice(-300));
          const first = await evaluate('window.__page');
          await evaluate("document.querySelector('button').click()");
          await evaluate("document.querySelector('button').click()");
          check(await until60(async () => (await text()) === 'count 2Hellochild one'), 'and its state changes');
          write2(app, 'App.vue', readFileSync(join(app, 'App.vue'), 'utf8').replace('Hello', 'Howdy'));
          check(await until60(async () => /Howdy/.test(await text())), 'an edited template is applied in place', String(await text()));
          check((await evaluate('window.__page')) === first, 'without a reload');
          const afterTemplate = await text();
          check(/count 2/.test(afterTemplate), 'and the state of the component is kept', String(afterTemplate));
          write2(app, 'Child.vue', readFileSync(join(app, 'Child.vue'), 'utf8').replace('child one', 'child two'));
          check(await until60(async () => /child two/.test(await text())), 'an edited child component is applied', String(await text()));
          check((await evaluate('window.__page')) === first, 'without a reload either');
          write2(app, 'App.vue', readFileSync(join(app, 'App.vue'), 'utf8').replace('ref(0)', 'ref(10)'));
          check(await until60(async () => /count 10/.test(await text())), 'an edited script re-creates the component', String(await text()));
          check((await evaluate('window.__page')) === first, 'and the page is still not reloaded');
        } finally {
          vue.kill();
        }
      }
      // Svelte 5: components through @sveltejs/vite-plugin-svelte (compiled with its own HMR, which uses import.meta.hot).
      if (existsSync(join(testModules, '@sveltejs', 'vite-plugin-svelte')) && existsSync(join(testModules, 'svelte'))) {
        const app = join(dir, 'svelte-app');
        cpSync(join(root, 'tests', 'hmr', 'svelte'), app, { recursive: true });
        symlinkSync(testModules, join(app, 'node_modules'), 'dir');
        const svelte = spawn(bin, ['serve', 'main.js', '--hmr', '--port', '0', '--define', 'process.env.NODE_ENV="development"'], { cwd: app, stdio: ['ignore', 'pipe', 'pipe'] });
        let svelteLog = '';
        svelte.stdout.on('data', (d) => (svelteLog += d));
        svelte.stderr.on('data', (d) => (svelteLog += d));
        try {
          if (!(await until(() => /(?:127\.0\.0\.1|localhost):\d+/.test(svelteLog)))) throw new Error('the Svelte server did not start: ' + svelteLog);
          const sveltePort = /(?:localhost|127\.0\.0\.1):(\d+)/.exec(svelteLog)[1];
          await fetchBuilt(`http://127.0.0.1:${sveltePort}`, () => svelteLog);
          await tab.navigate(`http://127.0.0.1:${sveltePort}/`);
          const text = () => evaluate("document.getElementById('app')?.textContent?.replace(/\\s+/g, ' ').trim()");
          check(await until60(async () => /count 0\s*Hello\s*child one/.test((await text()) ?? '')), 'the Svelte app renders', String(await text()) + ' ' + svelteLog.slice(-400));
          const first = await evaluate('window.__page');
          await evaluate("document.querySelector('button').click()");
          await evaluate("document.querySelector('button').click()");
          check(await until60(async () => /count 2/.test((await text()) ?? '')), 'and its state changes', String(await text()));
          write2(app, 'App.svelte', readFileSync(join(app, 'App.svelte'), 'utf8').replace('Hello', 'Howdy'));
          check(await until60(async () => /Howdy/.test((await text()) ?? '')), 'an edited template is applied in place', String(await text()) + ' ' + svelteLog.slice(-300));
          check((await evaluate('window.__page')) === first, 'without a reload');
          write2(app, 'Child.svelte', readFileSync(join(app, 'Child.svelte'), 'utf8').replace('child one', 'child two'));
          check(await until60(async () => /child two/.test((await text()) ?? '')), 'an edited child component is applied', String(await text()));
          check((await evaluate('window.__page')) === first, 'without a reload either');
          write2(app, 'App.svelte', readFileSync(join(app, 'App.svelte'), 'utf8').replace('$state(0)', '$state(10)'));
          check(await until60(async () => /count 10/.test((await text()) ?? '')), 'an edited script is applied', String(await text()));
          check((await evaluate('window.__page')) === first, 'and the page is still not reloaded');
        } finally {
          svelte.kill();
        }
      }
    } catch (error) {
      failed++;
      console.log('FAIL ' + (error?.stack ?? error));
    } finally {
      server.kill();
      await tab?.close();
    }
  }
}
// The message of a check that found a reload it did not expect: which rebuilds the server made after the edit, whether one failed, and what
// the page did with them, so a failure on a slow machine says what happened.
function explainReloadOf(pageEvents) {
  return (log, mark) => {
    const lines = log.slice(mark.log).split('\n').filter(Boolean);
    const rebuilds = lines.filter((l) => /^rebuilt in /.test(l)).length;
    const errors = lines.filter((l) => /^error/i.test(l) || /build error|failed/i.test(l));
    const seen = pageEvents.slice(mark.events);
    const why = errors.length ? 'a build error came first' : rebuilds > 1 ? 'a second rebuild came after the first' : rebuilds === 1 ? 'one rebuild, and the page asked for the reload itself' : 'no rebuild had finished';
    return `after the edit the server made ${rebuilds} rebuild(s); the reload is explained by: ${why}\n--- server log since the edit\n${lines.join('\n')}\n--- page since the edit\n${seen.join('\n')}`;
  };
}
// An edit lands whole, like the save of an editor that writes a temporary file and renames it. A plain writeFileSync empties the file first,
// and on a loaded machine the server can look at it in between: it builds a module with no exports (the page reloads, as for any module
// that stops being a component), and the full file then comes as a second rebuild. That was the flake of the React Refresh scenario.
function write2(base, name, text) {
  const temporary = join(base, `.${name}.${process.pid}.tmp`);
  writeFileSync(temporary, text);
  renameSync(temporary, join(base, name));
}
if (failed) {
  console.log(`${failed} failure(s)`);
  process.exit(1);
}
try { rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {} // Windows may still hold a file of a process that just ended
console.log(`hmr: all ${checks} checks pass${process.env.BROWSER ? ` (${process.env.BROWSER})` : ''}`);
