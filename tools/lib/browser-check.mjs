// Loads a built site in headless Chromium and reports what a visitor would hit: uncaught exceptions, console errors, files of the site
// that fail to load, and a page with nothing in it. No dependencies: the browser is driven over the DevTools protocol.
//
//   import { findBrowser, checkPage } from './lib/browser-check.mjs';
//   const result = await checkPage('/path/to/dist', { allowErrors: ['regex'], text: 'regex', selector: '#root' });
//   // { ok, problems: [string], text: 'what the page shows (first 200 characters)', html: 'the start of the body, to see what is there', log: ['console messages and outside requests'] }
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';

const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.ico': 'image/x-icon', '.woff': 'font/woff', '.woff2': 'font/woff2', '.wasm': 'application/wasm', '.map': 'application/json', '.txt': 'text/plain' };

/** The path of a Chromium-based browser, or null. `CHROME` names one; else Playwright's download or one on the PATH. */
export function findBrowser() {
  if (process.env.CHROME) return process.env.CHROME;
  const cache = path.join(os.homedir(), '.cache', 'ms-playwright');
  if (fs.existsSync(cache)) {
    for (const [prefix, rest] of [['chromium_headless_shell', ['chrome-headless-shell-linux64', 'chrome-headless-shell']], ['chromium-', ['chrome-linux64', 'chrome']]]) {
      for (const d of fs.readdirSync(cache).filter((d) => d.startsWith(prefix)).sort().reverse()) {
        const found = path.join(cache, d, ...rest);
        if (fs.existsSync(found)) return found;
      }
    }
  }
  for (const name of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'chrome']) if (spawnSync('which', [name], { encoding: 'utf8' }).status === 0) return name;
  return null;
}

const until = async (test, ms) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await test()) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
};

function serve(dir) {
  const server = http.createServer((req, res) => {
    const name = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    let file = path.join(dir, name);
    if (!file.startsWith(dir)) return res.writeHead(403).end();
    if (fs.existsSync(file) && fs.statSync(file).isDirectory()) file = path.join(file, 'index.html');
    // A single-page app answers every other address with its page, as hosting for one does.
    if (!fs.existsSync(file) && !path.extname(name)) file = path.join(dir, 'index.html');
    if (!fs.existsSync(file)) return res.writeHead(404).end('not found');
    res.writeHead(200, { 'content-type': types[path.extname(file)] ?? 'application/octet-stream' }).end(fs.readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port })));
}

/**
 * `dir`: the built site. `expect`: `allowErrors` (regular expressions, as text, for console errors that are fine), `text` (a regular
 * expression the page's text must match), `selector` (an element that must have children; default `body`).
 */
export async function checkPage(dir, expect = {}, browser = findBrowser()) {
  if (!browser) return { ok: true, skipped: true, problems: [], text: '' };
  const allowed = (expect.allowErrors ?? []).map((r) => new RegExp(r));
  const { server, port } = await serve(path.resolve(dir));
  const origin = `http://127.0.0.1:${port}`;
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'page-check-'));
  let chrome;
  const problems = [];
  const log = [];
  const note = (text) => {
    if (!allowed.some((r) => r.test(text)) && !problems.includes(text)) problems.push(text);
  };
  try {
    let targets;
    // A first launch can be slow: two tries of 30 seconds.
    for (let attempt = 0; attempt < 2 && !targets; attempt++) {
      chrome?.kill();
      chrome = spawn(browser, ['--no-sandbox', '--disable-gpu', '--headless=new', `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore' });
      await until(async () => {
        const f = path.join(profile, 'DevToolsActivePort');
        if (!fs.existsSync(f)) return false;
        const list = await fetch(`http://127.0.0.1:${fs.readFileSync(f, 'utf8').split('\n')[0]}/json`).then((r) => r.json()).catch(() => undefined);
        if (Array.isArray(list) && list.some((t) => t.type === 'page')) targets = list;
        return Boolean(targets);
      }, 30000);
    }
    if (!targets) return { ok: false, problems: ['the browser did not open a page to control'], text: '' };
    const ws = new WebSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
    await new Promise((r) => (ws.onopen = r));
    let id = 0;
    const pending = new Map();
    let loaded = false;
    const requests = new Map();
    ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      if (d.id && pending.has(d.id)) {
        pending.get(d.id)(d.result);
        pending.delete(d.id);
        return;
      }
      const p = d.params ?? {};
      switch (d.method) {
        case 'Page.loadEventFired':
          loaded = true;
          break;
        case 'Runtime.exceptionThrown':
          note(`uncaught: ${p.exceptionDetails?.exception?.description ?? p.exceptionDetails?.text}`.split('\n').slice(0, 3).join(' | '));
          break;
        case 'Runtime.consoleAPICalled':
          if (log.length < 20) log.push(`${p.type}: ${(p.args ?? []).map((a) => a.value ?? a.description ?? '').join(' ')}`.slice(0, 200));
          if (p.type === 'error') note(`console.error: ${(p.args ?? []).map((a) => a.value ?? a.description ?? '').join(' ')}`.slice(0, 300));
          break;
        case 'Network.requestWillBeSent':
          requests.set(p.requestId, p.request.url);
          // `node:module` and the like: a browser cannot load them, and the failure of a module graph is otherwise silent.
          if (!/^(https?|data|blob|about|chrome-extension):/.test(p.request.url)) note(`request for ${p.request.url.slice(0, 80)}, which a browser cannot load`);
          if (log.length < 20 && !p.request.url.startsWith(origin)) log.push(`request: ${p.request.url.slice(0, 120)}`);
          break;
        case 'Network.responseReceived':
          // Only the site's own files count; a page that calls an outside service gets what the network gives it.
          // A browser asks for /favicon.ico by itself; a page that has none is not broken.
          if (p.response.status >= 400 && p.response.url.startsWith(origin) && !p.response.url.endsWith('/favicon.ico')) note(`${p.response.status} for ${p.response.url.slice(origin.length)}`);
          break;
        case 'Network.loadingFailed':
          if ((requests.get(p.requestId) ?? '').startsWith(origin) && !p.canceled) note(`failed to load ${requests.get(p.requestId).slice(origin.length)}: ${p.errorText}`);
          break;
      }
    };
    const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
    const evaluate = async (expression) => (await send('Runtime.evaluate', { expression, returnByValue: true })).result?.value;
    await send('Runtime.enable');
    await send('Network.enable');
    await send('Page.enable');
    await send('Page.navigate', { url: `${origin}/` });
    if (!(await until(() => loaded, 30000))) note('the page did not finish loading in 30 seconds');
    // Scripts that start the page after load get a moment.
    const selector = JSON.stringify(expect.selector ?? 'body');
    await until(async () => (await evaluate(`(document.querySelector(${selector})?.children.length ?? 0) > 0 && document.body.innerText.trim().length > 0`)) === true, 5000);
    const text = String((await evaluate('document.body.innerText.trim()')) ?? '').replace(/\s+/g, ' ').slice(0, 200);
    const children = await evaluate(`document.querySelector(${selector})?.children.length ?? -1`);
    if (children === -1) note(`no element matches ${expect.selector}`);
    else if (!text && !(await evaluate('document.body.querySelectorAll("img,canvas,svg,video,input,button").length'))) note('the page shows nothing');
    if (expect.text && !new RegExp(expect.text).test(text)) note(`the page text does not match /${expect.text}/: ${text}`);
    const html = String((await evaluate('document.body.innerHTML')) ?? '').replace(/\s+/g, ' ').slice(0, 400);
    ws.close();
    return { ok: problems.length === 0, problems, text, html, log };
  } finally {
    if (chrome && chrome.exitCode === null) {
      const gone = new Promise((resolve) => chrome.once('exit', resolve));
      chrome.kill();
      await Promise.race([gone, new Promise((resolve) => setTimeout(resolve, 5000))]);
    }
    server.close();
    // The browser may still be writing to its folder for a moment; leaving a temporary folder behind is not a failure.
    try {
      fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    } catch {}
  }
}
