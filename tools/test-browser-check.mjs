// Checks tools/lib/browser-check.mjs on small sites made here: one that works and four that each break in a different way.
// Runs in the browser BROWSER names (chromium, firefox or webkit; see lib/browsers.mjs). Skipped when that browser is not installed.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { checkPage } from './lib/browser-check.mjs';
import { browserKind, findBrowser } from './lib/browsers.mjs';

const browser = findBrowser();
if (!browser) {
  console.log(`browser-check: no ${browserKind()} browser found (see tools/lib/browsers.mjs); skipped`);
  process.exit(0);
}
const base = fs.mkdtempSync(path.join(os.tmpdir(), 'browser-check-'));
const site = (name, html, script) => {
  fs.mkdirSync(path.join(base, name));
  fs.writeFileSync(path.join(base, name, 'index.html'), html);
  if (script !== undefined) fs.writeFileSync(path.join(base, name, 'a.js'), script);
};
const page = '<!doctype html><body><div id="root"></div><script type="module" src="/a.js"></script>';
// A page that has no favicon is fine (a full browser asks for one by itself; this asks the same way).
site('good', page, 'fetch("/favicon.ico").catch(() => {}); document.getElementById("root").innerHTML = "<h1>Hello</h1>"');
site('throws', page, 'throw new Error("boom")');
site('empty', '<!doctype html><body><div id="root"></div>');
site('missing', '<!doctype html><body><div id="root">hi</div><script type="module" src="/gone.js"></script>');
// An icon the page names but does not ship (a browser may or may not fetch it) is not a broken page; another missing file still is.
site('icon', '<!doctype html><head><link rel="icon" type="image/svg+xml" href="/gone.svg"></head><body><div id="root">hi</div></body>');
site('console', '<!doctype html><body><div id="root"><p>x</p></div><script type="module" src="/a.js"></script>', 'console.error("bad thing")');

let failed = 0;
const expect = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what} ${detail}`);
  }
};
const at = (name, e) => checkPage(path.join(base, name), e, browser);
let r = await at('good', { selector: '#root', text: 'Hello' });
expect(r.ok && r.text === 'Hello', 'a working page passes', JSON.stringify(r));
r = await at('throws');
expect(!r.ok && r.problems.some((p) => p.startsWith('uncaught: Error: boom')), 'an uncaught error is found', JSON.stringify(r));
r = await at('empty');
expect(!r.ok && r.problems.includes('the page shows nothing'), 'an empty page is found', JSON.stringify(r));
r = await at('missing');
expect(!r.ok && r.problems.includes('404 for /gone.js'), 'a missing file of the site is found', JSON.stringify(r));
r = await at('icon');
expect(r.ok, 'a missing icon the page names is not reported', JSON.stringify(r));
r = await at('console');
expect(!r.ok && r.problems.some((p) => p.includes('bad thing')), 'a console error is found', JSON.stringify(r));
r = await at('console', { allowErrors: ['bad thing'] });
expect(r.ok, 'an error that is allowed is not reported', JSON.stringify(r));
r = await at('good', { text: 'Goodbye' });
expect(!r.ok, 'text that is not on the page is found', JSON.stringify(r));
try {
  fs.rmSync(base, { recursive: true, force: true });
} catch {}
console.log(failed ? `${failed} failure(s)` : `browser-check agrees (${browserKind()})`);
process.exit(failed ? 1 : 0);
