// Tests for `rkp serve`: pages, built files and maps from memory, plugin middlewares and
// transformIndexHtml, and what the page is told when files change (style sheets swapped, reload, errors,
// plugin events, a reload a plugin stops).
//   node tools/run-serve.mjs
import { spawn } from 'node:child_process';
import net from 'node:net';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const dir = mkdtempSync(join(tmpdir(), 'serve-'));
const write = (name, text) => writeFileSync(join(dir, name), text);
write('main.js', 'import "./style.css";\nimport note from "./note.txt";\nconsole.log("main", note, import.meta.hot ? "hot" : "cold");\n');
write('style.css', 'body { color: red; }\n');
write('note.txt', 'first\n');
write('index.html', '<!doctype html><html><head><title>t</title></head><body><script type="module" src="/main.js"></script></body></html>\n');
write('rathkit.config.mjs', `
export default {
  plugins: [{
    name: 'dev-things',
    configureServer(server) {
      server.middlewares.use('/api', (req, res, next) => {
        if (req.url === '/hello') { res.setHeader('content-type', 'text/plain'); res.end('hello from a middleware'); }
        else next();
      });
    },
    transformIndexHtml(html) {
      return { html, tags: [{ tag: 'meta', attrs: { name: 'injected', content: 'yes' }, injectTo: 'head' }] };
    },
    handleHotUpdate({ file, server }) {
      if (file.endsWith('note.txt')) { server.ws.send('note-changed', { file: file.split(/[\\\\/]/).pop() }); return []; }
    },
  }],
};
`);

const child = spawn(bin, ['serve', 'main.js', '--port', '0'], { cwd: dir, stdio: ['ignore', 'pipe', 'pipe'] });
let stderr = '';
child.stderr.on('data', (d) => (stderr += d));
const base = await new Promise((resolve, reject) => {
  let out = '';
  child.stdout.on('data', (d) => {
    out += d;
    const m = out.match(/serving at (http:\/\/[^\s]+)/);
    if (m) resolve(m[1]);
  });
  child.on('exit', () => reject(new Error(`the server stopped\n${stderr}`)));
  setTimeout(() => reject(new Error('the server did not start')), 15000);
});

const get = async (path) => {
  const r = await fetch(base.replace(/\/$/, '') + path);
  return { status: r.status, text: await r.text(), type: r.headers.get('content-type') };
};
const check = (cond, what) => {
  if (!cond) throw new Error(what);
};
// Where IPv6 exists, `localhost` is answered on both loopback addresses: the first one it resolves to differs between machines, and a
// client that names 127.0.0.1 or [::1] must get an answer on every one of them.
const hasIPv6 = await new Promise((resolve) => {
  const probe = net.createServer().on('error', () => resolve(false));
  probe.listen(0, '::1', () => probe.close(() => resolve(true)));
});
let failed = null;
try {
  const port = new URL(base).port;
  const over = async (host) => (await fetch(`http://${host}:${port}/__rathkit/client.js`).catch((e) => ({ status: String(e.cause?.code ?? e) }))).status;
  check((await over('127.0.0.1')) === 200, 'the server answers on 127.0.0.1');
  if (hasIPv6) check((await over('[::1]')) === 200, `the server answers on [::1] (${await over('[::1]')})`);
  // Wait for the first build.
  for (let i = 0; i < 100; i++) {
    if ((await get('/main.js')).status === 200) break;
    await new Promise((r) => setTimeout(r, 50));
  }
  const page = await get('/');
  check(page.status === 200 && /text\/html/.test(page.type), 'the page is not served as html');
  check(page.text.includes('src="/bundle.js"'), `the entry script was not pointed at the build:\n${page.text}`);
  check(page.text.includes('/__rathkit/client.js'), 'the reload client is not in the page');
  check(page.text.includes('name="injected"'), 'transformIndexHtml did not run');
  check(page.text.includes('href="/bundle.css"'), 'the style sheet is not linked');
  const js = await get('/bundle.js');
  check(js.status === 200 && js.text.includes('"main"') && js.text.includes('__rathkit_hot'), 'bundle.js is wrong');
  check((await get('/bundle.js.map')).status === 200, 'no source map');
  check((await get('/bundle.css')).text.includes('color: red'), 'no style sheet');
  const api = await get('/api/hello');
  check(api.text === 'hello from a middleware', 'the middleware did not answer');
  check((await get('/api/other')).status === 404, 'an unhandled /api path should be 404');
  check((await get('/nope.js')).status === 404 && (await get('/../etc/passwd')).status !== 200, 'unknown or unsafe paths must not be served');
  check((await get('/__rathkit/client.js')).text.includes('EventSource'), 'no client');

  // Other sites must not be able to use the server through the visitor's browser.
  const raw = (head) =>
    new Promise((resolve) => {
      const url = new URL(base);
      const socket = net.connect(Number(url.port), url.hostname, () => socket.write(head + '\r\n\r\n'));
      let out = '';
      socket.on('data', (d) => (out += d));
      socket.on('close', () => resolve(out.split('\r\n')[0]));
      setTimeout(() => socket.destroy(), 3000);
    });
  check((await raw('GET /bundle.js HTTP/1.1\r\nHost: evil.example.com')).includes('403'), 'a foreign Host header must be refused');
  check((await raw('POST /__rathkit/send HTTP/1.1\r\nHost: localhost\r\nOrigin: http://evil.example.com\r\nContent-Length: 2\r\n\r\n{}')).includes('403'), 'a cross-origin POST must be refused');
  check((await get('/rathkit.config.mjs')).status === 404, 'the config file must not be served');

  // The page's event stream.
  const events = [];
  const controller = new AbortController();
  const stream = await fetch(base.replace(/\/$/, '') + '/__rathkit/events', { signal: controller.signal });
  (async () => {
    const decoder = new TextDecoder();
    let buffer = '';
    try {
      for await (const part of stream.body) {
        buffer += decoder.decode(part, { stream: true });
        let at;
        while ((at = buffer.indexOf('\n\n')) >= 0) {
          const line = buffer.slice(0, at).replace(/^data: /, '');
          buffer = buffer.slice(at + 2);
          events.push(JSON.parse(line));
        }
      }
    } catch {}
  })();
  const next = async (type, ms = 10000) => {
    const start = Date.now();
    while (Date.now() - start < ms) {
      const i = events.findIndex((e) => e.type === type || e.event === type);
      if (i >= 0) return events.splice(i, 1)[0];
      await new Promise((r) => setTimeout(r, 40));
    }
    throw new Error(`no "${type}" event; got ${JSON.stringify(events)}\n${stderr}`);
  };
  await next('connected');

  write('style.css', 'body { color: blue; }\n');
  const css = await next('css-update');
  check(css.files.includes('bundle.css'), 'css-update names the wrong file');
  check((await get('/bundle.css')).text.includes('blue'), 'the new style sheet is not served');

  write('main.js', 'import "./style.css";\nimport note from "./note.txt";\nconsole.log("changed", note);\n');
  await next('full-reload');
  check((await get('/bundle.js')).text.includes('"changed"'), 'the new script is not served');

  write('main.js', 'import "./style.css";\nconsole.log("broken"\n');
  const error = await next('error');
  check(/error|expected|unexpected/i.test(error.message), `error message: ${error.message}`);
  // The page that connects now is told about the error at once.
  write('main.js', 'import "./style.css";\nimport note from "./note.txt";\nconsole.log("fixed", note);\n');
  await next('full-reload');
  // Let the rebuild settle before the next write: on a slow runner a second rebuild of the file above can still be running, and a write that
  // lands in it is part of one batch with that file, which is reloaded as a whole (a failure seen on alpine, 2026-11-08).
  await new Promise((r) => setTimeout(r, 1000));
  events.length = 0;

  // A plugin takes over a file: its event arrives, and the page is not reloaded.
  write('note.txt', 'second\n');
  const custom = await next('custom');
  check(custom.event === 'note-changed' && custom.data.file === 'note.txt', `custom event: ${JSON.stringify(custom)}`);
  await new Promise((r) => setTimeout(r, 600));
  check(!events.some((e) => e.type === 'full-reload'), 'the page was reloaded although a plugin stopped it');
  controller.abort();
} catch (error) {
  failed = error;
}
child.kill('SIGKILL');
if (failed) {
  console.log(`FAIL serve: ${failed.message}`);
  process.exit(1);
}
console.log('serve: pages, files, middlewares, html hooks, css swaps, reloads, errors and plugin events all work');
// A killed server (and its plugin host) may still hold the folder for a moment on Windows, which cannot delete what a process is in.
try {
  rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
} catch (error) {
  if (process.platform !== 'win32') throw error;
}
