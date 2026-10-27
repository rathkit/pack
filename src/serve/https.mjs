// The HTTPS side of `rkp serve --https`: this process owns the public port, speaks TLS (HTTP/2, or HTTP/1.1 to clients that ask for it), and passes every request to the dev server
// (plain HTTP on the loopback), streams included, so server-sent events and hot updates work as they do without TLS.
//
// The certificate is the one given (`cert` and `key` files) or a self-signed one for localhost, made here and kept in the user's cache
// folder (the browser has to be told to trust it once). The process ends when its standard input closes, which is when the server's does.
//
// The request is JSON in the first argument: { host, port, upstream, cert, key, names }. It prints `listening <port>` when it is ready.
import fs from 'node:fs';
import http from 'node:http';
import http2 from 'node:http2';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import net from 'node:net';

const options = JSON.parse(process.argv[2]);

// ---- a self-signed certificate -------------------------------------------------------------------------

const der = (tag, ...parts) => {
  const body = Buffer.concat(parts);
  const length = body.length < 128 ? Buffer.from([body.length]) : body.length < 256 ? Buffer.from([0x81, body.length]) : Buffer.from([0x82, body.length >> 8, body.length & 255]);
  return Buffer.concat([Buffer.from([tag]), length, body]);
};
const oid = (text) => {
  const parts = text.split('.').map(Number);
  const bytes = [parts[0] * 40 + parts[1]];
  for (const part of parts.slice(2)) {
    const chunk = [part & 127];
    for (let rest = part >> 7; rest > 0; rest >>= 7) chunk.unshift((rest & 127) | 128);
    bytes.push(...chunk);
  }
  return der(0x06, Buffer.from(bytes));
};
const sequence = (...parts) => der(0x30, ...parts);
const utc = (date) => der(0x17, Buffer.from(date.toISOString().replace(/[-:T]/g, '').slice(2, 14) + 'Z'));
const ipBytes = (address) => {
  if (net.isIPv4(address)) return Buffer.from(address.split('.').map(Number));
  const [head, tail = ''] = address.split('::');
  const left = head ? head.split(':') : [];
  const right = tail ? tail.split(':') : [];
  const groups = [...left, ...Array(8 - left.length - right.length).fill('0'), ...right];
  return Buffer.concat(groups.map((g) => Buffer.from(g.padStart(4, '0'), 'hex')));
};

function selfSigned(names) {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  const name = sequence(der(0x31, sequence(oid('2.5.4.3'), der(0x0c, Buffer.from('localhost')))));
  const now = Date.now();
  const alt = names.map((n) => (net.isIP(n) ? der(0x87, ipBytes(n)) : der(0x82, Buffer.from(n))));
  const serial = crypto.randomBytes(16);
  serial[0] &= 0x7f;
  const signature = sequence(oid('1.2.840.10045.4.3.2'));
  const tbs = sequence(
    der(0xa0, der(0x02, Buffer.from([2]))),
    der(0x02, serial),
    signature,
    name,
    sequence(utc(new Date(now - 60_000)), utc(new Date(now + 365 * 86_400_000))),
    name,
    publicKey.export({ type: 'spki', format: 'der' }),
    der(0xa3, sequence(
      sequence(oid('2.5.29.19'), der(0x01, Buffer.from([0xff])), der(0x04, sequence())),
      sequence(oid('2.5.29.15'), der(0x01, Buffer.from([0xff])), der(0x04, der(0x03, Buffer.from([0x07, 0x80])))),
      sequence(oid('2.5.29.37'), der(0x04, sequence(oid('1.3.6.1.5.5.7.3.1')))),
      sequence(oid('2.5.29.17'), der(0x04, sequence(...alt))),
    )),
  );
  const signed = crypto.sign('sha256', tbs, { key: privateKey, dsaEncoding: 'der' });
  const certificate = sequence(tbs, signature, der(0x03, Buffer.concat([Buffer.from([0]), signed])));
  const pem = (label, bytes) => `-----BEGIN ${label}-----\n${bytes.toString('base64').replace(/(.{64})/g, '$1\n').trim()}\n-----END ${label}-----\n`;
  return { cert: pem('CERTIFICATE', certificate), key: privateKey.export({ type: 'pkcs8', format: 'pem' }) };
}

function credentials() {
  if (options.cert || options.key) {
    if (!options.cert || !options.key) throw new Error('--https-cert and --https-key go together');
    return { cert: fs.readFileSync(options.cert), key: fs.readFileSync(options.key) };
  }
  const folder = process.env.RATHKIT_CERT_DIR || path.join(process.env.XDG_CACHE_HOME || (process.platform === 'win32' ? process.env.LOCALAPPDATA : '') || path.join(os.homedir(), '.cache'), 'rathkit');
  const certFile = path.join(folder, 'dev-cert.pem');
  const keyFile = path.join(folder, 'dev-key.pem');
  const wanted = ['localhost', '127.0.0.1', '::1', ...options.names];
  try {
    const cert = fs.readFileSync(certFile, 'utf8');
    const x509 = new crypto.X509Certificate(cert);
    const stillGood = new Date(x509.validTo).getTime() - Date.now() > 30 * 86_400_000;
    const covers = wanted.every((n) => (net.isIP(n) ? x509.checkIP(n) : x509.checkHost(n)));
    if (stillGood && covers) return { cert, key: fs.readFileSync(keyFile, 'utf8') };
  } catch {}
  const made = selfSigned([...new Set(wanted)]);
  try {
    fs.mkdirSync(folder, { recursive: true, mode: 0o700 });
    fs.writeFileSync(certFile, made.cert, { mode: 0o600 });
    fs.writeFileSync(keyFile, made.key, { mode: 0o600 });
  } catch (error) {
    process.stderr.write(`warning: the certificate could not be kept in ${folder} (${error.message}); it is made again next time\n`);
  }
  return made;
}

// ---- the proxy -------------------------------------------------------------------------------------------

const { cert, key } = credentials();
const upstream = { host: '127.0.0.1', port: options.upstream };
// What belongs to one HTTP/1.1 connection only, which HTTP/2 does not carry.
const hopByHop = new Set(['connection', 'keep-alive', 'proxy-connection', 'transfer-encoding', 'upgrade', 'te']);
// HTTP/2 and HTTP/1.1 on the same port: browsers take HTTP/2 (many module requests share a connection), other clients stay on HTTP/1.1.
const server = http2.createSecureServer({ cert, key, allowHTTP1: true }, (req, res) => {
  const headers = {};
  for (const [name, value] of Object.entries(req.headers)) if (!name.startsWith(':') && !hopByHop.has(name)) headers[name] = value;
  // HTTP/2 names the host in :authority.
  if (req.headers[':authority'] && !headers.host) headers.host = req.headers[':authority'];
  headers['x-forwarded-proto'] = 'https';
  const forward = http.request({ ...upstream, method: req.method, path: req.url, headers }, (answer) => {
    const out = {};
    for (const [name, value] of Object.entries(answer.headers)) if (req.httpVersionMajor < 2 || !hopByHop.has(name)) out[name] = value;
    res.writeHead(answer.statusCode, out);
    answer.pipe(res);
  });
  forward.on('error', (error) => {
    if (!res.headersSent) res.writeHead(502, { 'content-type': 'text/plain' });
    res.end('Bad gateway: ' + error.message);
  });
  req.pipe(forward);
  // A page that goes away ends its request to the server (an event stream would otherwise stay).
  res.on('close', () => forward.destroy());
});
server.on('clientError', (_error, socket) => socket.destroy());
server.on('sessionError', () => {});
server.listen(options.port, options.host, () => process.stdout.write(`listening ${server.address().port}\n`));
server.on('error', (error) => {
  process.stdout.write(`error ${error.message}\n`);
  process.exit(1);
});
process.stdin.resume();
process.stdin.on('end', () => process.exit(0));
process.stdin.on('error', () => process.exit(0));
