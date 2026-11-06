// Drives one page of a headless browser, with no dependencies: Chromium over the DevTools protocol, Firefox over WebDriver BiDi and
// WebKit over the inspector pipe of Playwright's build. The tools that need a browser (the page check, HMR, federation) use this, so
// each of them runs in all three.
//
//   import { browserKind, findBrowser, openBrowser } from './lib/browsers.mjs';
//   const kind = browserKind();                   // BROWSER=chromium (default) | firefox | webkit
//   const binary = findBrowser(kind);             // null when that browser is not installed
//   const page = await openBrowser(kind, binary);
//   page.onEvent((e) => ...);                     // set before navigate; see "Events"
//   await page.navigate('http://127.0.0.1:8080/');
//   await page.evaluate('document.title');       // the value of an expression (strings, numbers, booleans, null, undefined)
//   await page.close();
//
// Events: { type: 'load' } | { type: 'console', level, text } (console.* calls only) | { type: 'exception', text } (an uncaught error)
//   | { type: 'request', id, url } | { type: 'response', url, status } | { type: 'failed', url, text, canceled }.
//
// Where the browsers come from: CHROME, FIREFOX and WEBKIT name one (WEBKIT is Playwright's `MiniBrowser` launcher script); otherwise
// the folder Playwright downloads to (`pnpm dlx playwright@VERSION install chromium firefox webkit`), then the PATH.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const kinds = ['chromium', 'firefox', 'webkit'];

/** The browser the tools use: the BROWSER variable, `chromium` when it is not set. */
export function browserKind() {
  const kind = (process.env.BROWSER ?? 'chromium').toLowerCase();
  if (!kinds.includes(kind)) throw new Error(`BROWSER is "${process.env.BROWSER}"; it is one of ${kinds.join(', ')}`);
  return kind;
}

const cache = () => path.join(os.homedir(), '.cache', 'ms-playwright');
/** The newest Playwright download whose folder starts with `prefix` and holds `rest`. */
function download(prefix, rest) {
  if (!fs.existsSync(cache())) return null;
  for (const d of fs.readdirSync(cache()).filter((d) => d.startsWith(prefix)).sort().reverse()) {
    const found = path.join(cache(), d, ...rest);
    if (fs.existsSync(found)) return found;
  }
  return null;
}
const onPath = (names) => names.find((name) => spawnSync('which', [name], { encoding: 'utf8' }).status === 0) ?? null;

/** The path of the browser of that kind, or null when it is not installed. */
export function findBrowser(kind = browserKind()) {
  if (kind === 'chromium') {
    return process.env.CHROME || download('chromium_headless_shell', ['chrome-headless-shell-linux64', 'chrome-headless-shell']) || download('chromium-', ['chrome-linux64', 'chrome']) || onPath(['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'chrome']);
  }
  if (kind === 'firefox') return process.env.FIREFOX || download('firefox-', ['firefox', 'firefox']) || onPath(['firefox']);
  // Playwright's WebKit is the only one that can be driven this way; it is built for Linux here.
  return process.env.WEBKIT || (process.platform === 'linux' ? download('webkit-', ['minibrowser-wpe', 'MiniBrowser']) : null);
}

const until = async (test, ms) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await test()) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
};
const withSocket = (url) => new Promise((resolve, reject) => {
  const ws = new WebSocket(url);
  ws.onopen = () => resolve(ws);
  ws.onerror = () => reject(new Error(`could not connect to ${url}`));
});
/** Sends requests and matches their answers: `call(method, params)` resolves with the message that answers it. */
function requests(write) {
  let id = 0;
  const waiting = new Map();
  return {
    call: (method, params = {}, extra = {}) => new Promise((resolve) => {
      const n = ++id;
      waiting.set(n, resolve);
      write({ id: n, method, params }, extra);
    }),
    answer(message) {
      if (!message.id || !waiting.has(message.id)) return false;
      waiting.get(message.id)(message);
      waiting.delete(message.id);
      return true;
    },
    /** Resolves every request still waiting (the browser went away) with an error. */
    drop(why) {
      for (const [n, resolve] of waiting) resolve({ id: n, error: { message: why } });
      waiting.clear();
    },
  };
}
/** Ends the browser: asks politely, waits a few seconds, then insists, so a slow shutdown never leaves one running. */
const stop = async (child) => {
  if (child.exitCode !== null) return;
  const ended = new Promise((resolve) => child.once('exit', resolve));
  child.kill();
  const timer = new Promise((resolve) => setTimeout(() => resolve('late'), 5000));
  if ((await Promise.race([ended, timer])) === 'late') {
    child.kill('SIGKILL');
    await Promise.race([ended, new Promise((resolve) => setTimeout(resolve, 5000))]);
  }
};

// ---- Chromium: the DevTools protocol over a WebSocket ----
async function chromium(binary, emit) {
  let child, targets, profile;
  // A first launch can be slow: two tries of 30 seconds.
  for (let attempt = 0; attempt < 2 && !targets; attempt++) {
    child?.kill();
    if (profile) try { fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); } catch {} // a browser that was just killed may still be writing
    profile = fs.mkdtempSync(path.join(os.tmpdir(), 'page-profile-'));
    child = spawn(binary, ['--no-sandbox', '--disable-gpu', '--headless=new', `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore' });
    await until(async () => {
      const f = path.join(profile, 'DevToolsActivePort');
      if (!fs.existsSync(f)) return false;
      const list = await fetch(`http://127.0.0.1:${Number.parseInt(fs.readFileSync(f, 'utf8'), 10)}/json`).then((r) => r.json()).catch(() => undefined);
      if (Array.isArray(list) && list.some((t) => t.type === 'page')) targets = list;
      return Boolean(targets);
    }, 30000);
  }
  if (!targets) {
    child.kill();
    throw new Error('the browser did not open a page to control');
  }
  const ws = await withSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
  const rpc = requests((m) => ws.send(JSON.stringify(m)));
  const urls = new Map();
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (rpc.answer(d)) return;
    const p = d.params ?? {};
    const text = (args) => (args ?? []).map((a) => a.value ?? a.description ?? '').join(' ');
    switch (d.method) {
      case 'Page.loadEventFired':
        return emit({ type: 'load' });
      case 'Runtime.exceptionThrown':
        return emit({ type: 'exception', text: p.exceptionDetails?.exception?.description ?? p.exceptionDetails?.text ?? '' });
      case 'Runtime.consoleAPICalled':
        return emit({ type: 'console', level: p.type, text: text(p.args) });
      case 'Network.requestWillBeSent':
        urls.set(p.requestId, p.request.url);
        return emit({ type: 'request', id: p.requestId, url: p.request.url });
      case 'Network.responseReceived':
        return emit({ type: 'response', url: p.response.url, status: p.response.status });
      case 'Network.loadingFailed':
        return emit({ type: 'failed', url: urls.get(p.requestId) ?? '', text: p.errorText, canceled: Boolean(p.canceled) });
    }
  };
  for (const m of ['Runtime.enable', 'Network.enable', 'Page.enable']) await rpc.call(m);
  return {
    navigate: (url) => rpc.call('Page.navigate', { url }),
    evaluate: async (expression) => (await rpc.call('Runtime.evaluate', { expression, returnByValue: true })).result?.result?.value,
    async close() {
      ws.close();
      await stop(child);
      try {
        fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
      } catch {}
    },
  };
}

// ---- Firefox: WebDriver BiDi over a WebSocket ----
/** A BiDi remote value as plain data (primitives; arrays and objects of them). */
function plain(v) {
  switch (v?.type) {
    case 'string': case 'boolean': case 'bigint': return v.value;
    case 'number': return typeof v.value === 'string' ? Number(v.value) : v.value; // NaN, Infinity and -0 come as text
    case 'null': return null;
    case 'array': return (v.value ?? []).map(plain);
    case 'object': return Object.fromEntries((v.value ?? []).map(([k, x]) => [typeof k === 'string' ? k : plain(k), plain(x)]));
    default: return undefined;
  }
}
async function firefox(binary, emit) {
  let child, profile, port;
  for (let attempt = 0; attempt < 2 && !port; attempt++) {
    child?.kill();
    if (profile) try { fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); } catch {} // a browser that was just killed may still be writing
    profile = fs.mkdtempSync(path.join(os.tmpdir(), 'page-profile-'));
    // Quiet the first-run work, which would only add network calls and windows.
    fs.writeFileSync(path.join(profile, 'user.js'), [
      'browser.shell.checkDefaultBrowser', 'app.update.enabled', 'extensions.update.enabled', 'datareporting.policy.dataSubmissionEnabled',
      'toolkit.telemetry.enabled', 'network.captive-portal-service.enabled', 'browser.aboutwelcome.enabled', 'datareporting.healthreport.uploadEnabled',
    ].map((name) => `user_pref("${name}", false);`).join('\n'));
    child = spawn(binary, ['--headless', '--no-remote', '--remote-debugging-port=0', '--profile', profile, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
    let said = '';
    child.stderr.on('data', (d) => (said += d));
    await until(() => {
      port = /WebDriver BiDi listening on ws:\/\/127\.0\.0\.1:(\d+)/.exec(said)?.[1];
      return Boolean(port);
    }, 30000);
  }
  if (!port) {
    child.kill();
    throw new Error('the browser did not open a page to control');
  }
  const ws = await withSocket(`ws://127.0.0.1:${port}/session`);
  const rpc = requests((m) => ws.send(JSON.stringify(m)));
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (rpc.answer(d)) return;
    const p = d.params ?? {};
    switch (d.method) {
      case 'browsingContext.load':
        return emit({ type: 'load' });
      case 'log.entryAdded':
        // `javascript` entries are uncaught errors; `console` ones are calls of console.*.
        if (p.type === 'javascript') return emit({ type: 'exception', text: p.text });
        if (p.type === 'console') return emit({ type: 'console', level: p.method === 'warn' ? 'warning' : p.method, text: (p.args ?? []).map((a) => plain(a) ?? a.type).join(' ') });
        return;
      case 'network.beforeRequestSent':
        return emit({ type: 'request', id: p.request.request, url: p.request.url });
      case 'network.responseStarted':
        return emit({ type: 'response', url: p.response.url, status: p.response.status });
      case 'network.fetchError':
        return emit({ type: 'failed', url: p.request.url, text: p.errorText, canceled: /ABORT/.test(p.errorText ?? '') });
    }
  };
  const ok = (r) => {
    if (r.error) throw new Error(`${r.error}: ${r.message ?? ''}`);
    return r.result;
  };
  ok(await rpc.call('session.new', { capabilities: {} }));
  const context = ok(await rpc.call('browsingContext.getTree', {})).contexts[0].context;
  ok(await rpc.call('session.subscribe', { events: ['browsingContext.load', 'log.entryAdded', 'network.beforeRequestSent', 'network.responseStarted', 'network.fetchError'] }));
  return {
    navigate: (url) => rpc.call('browsingContext.navigate', { context, url, wait: 'none' }),
    async evaluate(expression) {
      const r = await rpc.call('script.evaluate', { expression, target: { context }, awaitPromise: false, resultOwnership: 'none' });
      return r.result?.type === 'success' ? plain(r.result.result) : undefined;
    },
    async close() {
      ws.close();
      await stop(child);
      try {
        fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
      } catch {}
    },
  };
}

// ---- WebKit: Playwright's build speaks its own protocol, as NUL-separated JSON on file descriptors 3 (to it) and 4 (from it) ----
async function webkit(binary, emit) {
  const child = spawn(binary, ['--inspector-pipe', '--headless', '--no-startup-window'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
  child.stdio[3].on('error', () => {});
  const browser = requests((m, extra) => child.stdio[3].write(JSON.stringify({ ...m, ...extra }) + '\0'));
  // The page's own commands travel inside `Target.sendMessageToTarget`, with their own numbering and answers.
  let pageProxyId, current;
  const urls = new Map();
  const pageRpc = requests((m, { targetId }) => browser.call('Target.sendMessageToTarget', { message: JSON.stringify(m), targetId }, { pageProxyId }));
  const pageCall = (targetId, method, params) => pageRpc.call(method, params, { targetId });
  const initialized = new Map();
  const initialize = async (targetId) => {
    for (const m of ['Page.enable', 'Runtime.enable', 'Console.enable', 'Network.enable']) await pageCall(targetId, m);
    await browser.call('Target.resume', { targetId }, { pageProxyId });
  };
  let buffer = '';
  child.stdio[4].on('data', (d) => {
    buffer += d;
    let i;
    while ((i = buffer.indexOf('\0')) >= 0) {
      const m = JSON.parse(buffer.slice(0, i));
      buffer = buffer.slice(i + 1);
      if (browser.answer(m)) continue;
      switch (m.method) {
        case 'Playwright.pageProxyCreated':
          pageProxyId = m.params.pageProxyId;
          break;
        case 'Target.targetCreated':
          if (m.params.targetInfo.type === 'page') {
            // A page that is paused is set up first and then let go; a new one for the same page appears when navigation swaps processes.
            if (m.params.targetInfo.isPaused) initialized.set(m.params.targetInfo.targetId, initialize(m.params.targetInfo.targetId));
            current ??= m.params.targetInfo.targetId;
          }
          break;
        case 'Target.didCommitProvisionalTarget':
          current = m.params.newTargetId;
          break;
        case 'Target.dispatchMessageFromTarget': {
          const t = JSON.parse(m.params.message);
          if (pageRpc.answer(t)) break;
          const p = t.params ?? {};
          switch (t.method) {
            case 'Page.loadEventFired':
              emit({ type: 'load' });
              break;
            case 'Console.messageAdded':
              // `javascript` messages are uncaught errors; `console-api` ones are calls of console.*. The rest (a failed load, say) have their own events.
              if (p.message.source === 'javascript' && p.message.level === 'error') emit({ type: 'exception', text: p.message.text });
              else if (p.message.source === 'console-api') emit({ type: 'console', level: p.message.level, text: p.message.text });
              break;
            case 'Network.requestWillBeSent':
              urls.set(p.requestId, p.request.url);
              emit({ type: 'request', id: p.requestId, url: p.request.url });
              break;
            case 'Network.responseReceived':
              emit({ type: 'response', url: p.response.url, status: p.response.status });
              break;
            case 'Network.loadingFailed':
              emit({ type: 'failed', url: urls.get(p.requestId) ?? '', text: p.errorText, canceled: Boolean(p.canceled) });
              break;
          }
        }
      }
    }
  });
  child.once('exit', () => {
    browser.drop('the browser ended');
    pageRpc.drop('the browser ended');
  });
  const ok = (r) => {
    if (r.error) throw new Error(r.error.message);
    return r.result;
  };
  ok(await browser.call('Playwright.enable'));
  const { browserContextId } = ok(await browser.call('Playwright.createContext'));
  ok(await browser.call('Playwright.createPage', { browserContextId }));
  if (!(await until(() => initialized.has(current), 10000))) throw new Error('the browser did not open a page to control');
  await initialized.get(current);
  const frameId = ok(await pageCall(current, 'Page.getResourceTree')).frameTree.frame.id;
  return {
    navigate: (url) => browser.call('Playwright.navigate', { url, pageProxyId, frameId }),
    async evaluate(expression) {
      const r = await pageCall(current, 'Runtime.evaluate', { expression, returnByValue: true });
      return r.result?.wasThrown ? undefined : r.result?.result?.value;
    },
    async close() {
      await stop(child);
    },
  };
}

/** Starts the browser and opens a blank page; see the top of this file for what the result does. */
export async function openBrowser(kind = browserKind(), binary = findBrowser(kind)) {
  if (!binary) throw new Error(`no ${kind} browser found`);
  let handler = () => {};
  const emit = (event) => handler(event);
  const page = await { chromium, firefox, webkit }[kind](binary, emit);
  return { ...page, kind, onEvent: (fn) => (handler = fn) };
}
