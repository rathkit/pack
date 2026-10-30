// Tests for `rkp serve --https`: the generated certificate is valid for localhost and loopback addresses and is kept between runs, the
// page and the event stream (which carries hot updates) come through TLS, and a given certificate and key are used.
//   node tools/run-https.mjs
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync, copyFileSync, rmSync, existsSync } from 'node:fs';
import https from 'node:https';
import net from 'node:net';
import http2 from 'node:http2';
import crypto from 'node:crypto';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const dir = mkdtempSync(join(tmpdir(), 'https-'));
const certs = join(dir, 'certs');
writeFileSync(join(dir, 'index.html'), '<!doctype html><html><body><script type="module" src="/main.js"></script></body></html>\n');
writeFileSync(join(dir, 'main.js'), 'console.log("https page");\n');
let failed = 0;
const check = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n  ' + detail : ''}`);
  }
};
const until = async (condition, ms = 20000) => {
  const started = Date.now();
  while (Date.now() - started < ms) {
    if (await condition()) return true;
    await new Promise((r) => setTimeout(r, 50));
  }
  return false;
};
const get = (port, path, options = {}) => new Promise((resolve, reject) => {
  const req = https.get({ host: '127.0.0.1', servername: 'localhost', port, path, ...options }, (res) => {
    let body = '';
    res.on('data', (d) => (body += d));
    res.on('end', () => resolve({ status: res.statusCode, body }));
  });
  req.on('error', reject);
});
const serve = async (args, env = {}) => {
  const child = spawn(bin, ['serve', 'main.js', '--port', '0', ...args], { cwd: dir, env: { ...process.env, RATHKIT_CERT_DIR: certs, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  child.stdout.on('data', (d) => (log += d));
  child.stderr.on('data', (d) => (log += d));
  await until(() => /serving at https?:\/\/[^\s]+:\d+/.test(log) || /error/.test(log));
  const port = Number(/serving at https?:\/\/[^\s/]+:(\d+)/.exec(log)?.[1]);
  return { child, port, log: () => log };
};
const fingerprint = (pem) => new crypto.X509Certificate(pem).fingerprint256;

try {
  // The generated certificate.
  let server = await serve(['--https']);
  check(/serving at https:\/\/localhost:\d+\//.test(server.log()), 'it says it serves https', server.log());
  const pem = readFileSync(join(certs, 'dev-cert.pem'), 'utf8');
  const x509 = new crypto.X509Certificate(pem);
  check(x509.checkHost('localhost') && x509.checkIP('127.0.0.1') && x509.checkIP('::1'), 'the certificate is for localhost and the loopback addresses', x509.subjectAltName);
  check(new Date(x509.validTo) - Date.now() > 300 * 86_400_000 && new Date(x509.validFrom) < new Date(), 'and valid for most of a year');
  check(x509.verify(x509.publicKey), 'and signed by its own key');
  check(readFileSync(join(certs, 'dev-key.pem'), 'utf8').includes('PRIVATE KEY'), 'the key is kept beside it');

  // Trusting it, a client gets the page; not trusting it, it does not.
  await until(async () => (await get(server.port, '/', { ca: pem }).catch(() => ({ status: 0 }))).status === 200);
  const page = await get(server.port, '/', { ca: pem });
  check(page.status === 200 && page.body.includes('/__rathkit/client.js'), 'the page comes through TLS', page.body.slice(0, 200));
  const untrusted = await get(server.port, '/').then(() => 'accepted', (e) => e.code);
  check(/SELF_SIGNED|UNABLE_TO_VERIFY/.test(untrusted), 'a client that does not trust the certificate refuses it', untrusted);
  const built = await get(server.port, '/bundle.js', { ca: pem });
  check(built.status === 200 && built.body.includes('https page'), 'built files come through', String(built.status));

  // The event stream, which a page keeps open for reloads and hot updates, is passed on as it happens.
  const messages = [];
  const events = https.get({ host: '127.0.0.1', servername: 'localhost', port: server.port, path: '/__rathkit/events', ca: pem }, (res) => {
    res.on('data', (d) => {
      for (const part of d.toString().split('\n\n')) if (part.startsWith('data: ')) messages.push(JSON.parse(part.slice(6)));
    });
  });
  check(await until(() => messages.some((m) => m.type === 'connected')), 'the event stream connects', JSON.stringify(messages));
  writeFileSync(join(dir, 'main.js'), 'console.log("changed");\n');
  check(await until(() => messages.some((m) => m.type === 'full-reload' || m.type === 'hmr')), 'and carries a rebuild as it happens', JSON.stringify(messages));
  events.destroy();

  // HTTP/2 for clients that take it: requests share one connection, and the event stream is one of its streams.
  {
    const session = http2.connect(`https://localhost:${server.port}`, { ca: pem });
    const request = (path) => new Promise((resolve, reject) => {
      const stream = session.request({ ':path': path });
      let status;
      let body = '';
      stream.on('response', (headers) => (status = headers[':status']));
      stream.on('data', (d) => (body += d));
      stream.on('end', () => resolve({ status, body }));
      stream.on('error', reject);
      stream.end();
    });
    const [page2, bundle2, missing] = await Promise.all([request('/'), request('/bundle.js'), request('/nope.txt')]);
    check(session.alpnProtocol === 'h2', 'a client that asks for HTTP/2 gets it', String(session.alpnProtocol));
    check(page2.status === 200 && page2.body.includes('/__rathkit/client.js') && bundle2.status === 200 && bundle2.body.includes('changed') && missing.status === 404, 'and its requests are answered, side by side', JSON.stringify([page2.status, bundle2.status, missing.status]));
    const seen = [];
    const events2 = session.request({ ':path': '/__rathkit/events' });
    events2.on('data', (d) => {
      for (const part of d.toString().split('\n\n')) if (part.startsWith('data: ')) seen.push(JSON.parse(part.slice(6)));
    });
    check(await until(() => seen.some((m) => m.type === 'connected')), 'the event stream is a stream of the same connection', JSON.stringify(seen));
    writeFileSync(join(dir, 'main.js'), 'console.log("changed again");\n');
    check(await until(() => seen.some((m) => m.type === 'full-reload' || m.type === 'hmr')), 'and it carries a rebuild', JSON.stringify(seen));
    events2.close();
    session.close();
  }
  // Plain HTTP to the same port is not served.
  // Both loopback addresses answer (where IPv6 exists), whichever one `localhost` resolves to first on the machine.
  const v6 = await new Promise((resolve) => {
    const probe = net.createServer().on('error', () => resolve(false));
    probe.listen(0, '::1', () => probe.close(() => resolve(true)));
  });
  for (const host of v6 ? ['127.0.0.1', '::1'] : ['127.0.0.1']) {
    const answered = await get(server.port, '/', { ca: pem, host }).then((r) => r.status, (e) => e.code);
    check(answered === 200, `https answers on ${host}`, String(answered));
  }
  const plain = await fetch(`http://127.0.0.1:${server.port}/`).then((r) => `status ${r.status}`, () => 'refused');
  check(plain === 'refused', 'plain HTTP is not answered on the TLS port', plain);
  server.child.kill();

  // The certificate is kept: the next run uses the same one.
  server = await serve(['--https']);
  await until(async () => (await get(server.port, '/', { ca: pem }).catch(() => ({ status: 0 }))).status === 200);
  check(fingerprint(readFileSync(join(certs, 'dev-cert.pem'), 'utf8')) === fingerprint(pem), 'the same certificate is used next time');
  server.child.kill();

  // A certificate and key of one's own.
  copyFileSync(join(certs, 'dev-cert.pem'), join(dir, 'mine.pem'));
  copyFileSync(join(certs, 'dev-key.pem'), join(dir, 'mine.key'));
  rmSync(certs, { recursive: true, force: true });
  server = await serve(['--https-cert', 'mine.pem', '--https-key', 'mine.key']);
  const given = await until(async () => (await get(server.port, '/', { ca: pem }).catch(() => ({ status: 0 }))).status === 200);
  check(given && !existsSync(join(certs, 'dev-cert.pem')), 'a given certificate is used, and none is made', server.log());
  server.child.kill();

  // A certificate without its key is refused.
  server = await serve(['--https-cert', 'mine.pem']);
  check(/error/.test(server.log()), 'a certificate without a key is an error', server.log());
  server.child.kill();

  // Only for serve.
  const bundle = spawnSync(bin, ['bundle', 'main.js', '--https', '--outfile', join(dir, 'x.js')], { cwd: dir, encoding: 'utf8' });
  check(bundle.status !== 0 && /--https is for rkp serve/.test(bundle.stderr), '--https is for serve only', bundle.stderr);
} catch (error) {
  failed++;
  console.log('FAIL ' + (error?.stack ?? error));
} finally {
  rmSync(dir, { recursive: true, force: true });
}
if (failed) {
  console.log(`${failed} failure(s)`);
  process.exit(1);
}
console.log('https: all checks pass');
