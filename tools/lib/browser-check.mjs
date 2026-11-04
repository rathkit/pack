// Loads a built site in a headless browser (Chromium, Firefox or WebKit; BROWSER picks, see browsers.mjs) and reports what a visitor
// would hit: uncaught exceptions, console errors, files of the site that fail to load, and a page with nothing in it. No dependencies.
//
//   import { findBrowser, checkPage } from './lib/browser-check.mjs';
//   const result = await checkPage('/path/to/dist', { allowErrors: ['regex'], text: 'regex', selector: '#root' });
//   // { ok, problems: [string], text: 'what the page shows (first 200 characters)', html: 'the start of the body, to see what is there', log: ['console messages and outside requests'] }
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { findBrowser, openBrowser, browserKind } from './browsers.mjs';

const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.ico': 'image/x-icon', '.woff': 'font/woff', '.woff2': 'font/woff2', '.wasm': 'application/wasm', '.map': 'application/json', '.txt': 'text/plain' };

export { findBrowser };

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
  const problems = [];
  const log = [];
  const note = (text) => {
    if (!allowed.some((r) => r.test(text)) && !problems.includes(text)) problems.push(text);
  };
  let page;
  try {
    try {
      page = await openBrowser(browserKind(), browser);
    } catch (error) {
      return { ok: false, problems: [error.message], text: '' };
    }
    let loaded = false;
    const requests = new Map();
    page.onEvent((e) => {
      switch (e.type) {
        case 'load':
          loaded = true;
          break;
        case 'exception':
          note(`uncaught: ${e.text}`.split('\n').slice(0, 3).join(' | '));
          break;
        case 'console':
          if (log.length < 20) log.push(`${e.level}: ${e.text}`.slice(0, 200));
          if (e.level === 'error') note(`console.error: ${e.text}`.slice(0, 300));
          break;
        case 'request':
          requests.set(e.id, e.url);
          // `node:module` and the like: a browser cannot load them, and the failure of a module graph is otherwise silent.
          if (!/^(https?|data|blob|about|chrome-extension|moz-extension):/.test(e.url)) note(`request for ${e.url.slice(0, 80)}, which a browser cannot load`);
          if (log.length < 20 && !e.url.startsWith(origin)) log.push(`request: ${e.url.slice(0, 120)}`);
          break;
        case 'response':
          // Only the site's own files count; a page that calls an outside service gets what the network gives it.
          // A browser asks for /favicon.ico by itself; a page that has none is not broken.
          if (e.status >= 400 && e.url.startsWith(origin) && !e.url.endsWith('/favicon.ico')) note(`${e.status} for ${e.url.slice(origin.length)}`);
          break;
        case 'failed':
          if (e.url.startsWith(origin) && !e.canceled) note(`failed to load ${e.url.slice(origin.length)}: ${e.text}`);
          break;
      }
    });
    const evaluate = (expression) => page.evaluate(expression);
    await page.navigate(`${origin}/`);
    if (!(await until(() => loaded, 30000))) note('the page did not finish loading in 30 seconds');
    // Scripts that start the page after load get a moment.
    const selector = JSON.stringify(expect.selector ?? 'body');
    await until(async () => (await evaluate(`(document.querySelector(${selector})?.children.length ?? 0) > 0 && document.body.innerText.trim().length > 0`)) === true, 5000);
    const text = String((await evaluate('document.body.innerText.trim()')) ?? '').replace(/\s+/g, ' ').slice(0, 200);
    const children = await evaluate(`document.querySelector(${selector})?.children.length ?? -1`);
    if (children === -1) note(`no element matches ${expect.selector}`);
    else if (!expect.empty && !text && !(await evaluate('document.body.querySelectorAll("img,canvas,svg,video,input,button").length'))) note('the page shows nothing');
    if (expect.text && !new RegExp(expect.text).test(text)) note(`the page text does not match /${expect.text}/: ${text}`);
    const html = String((await evaluate('document.body.innerHTML')) ?? '').replace(/\s+/g, ' ').slice(0, 400);
    return { ok: problems.length === 0, problems, text, html, log };
  } finally {
    await page?.close();
    server.close();
  }
}
