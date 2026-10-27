// Tests for plugin objects given to the JS API: hooks run in this process (their state is kept here), `this.resolve` and `this.warn`
// work across the socket, what a plugin changes in the output is applied, a plugin can be a module to load, a config file still applies,
// and what cannot work says so.
//   node tools/test-plugin-objects.mjs
import { createRequire } from 'node:module';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
process.env.RATHKIT_PACK_BIN ??= join(root, 'target', 'release', 'rkp');
const { build, serve } = createRequire(import.meta.url)(join(root, 'npm', 'pack', 'index.js'));
const dir = mkdtempSync(join(tmpdir(), 'plugin-objects-'));
const write = (name, text) => {
  mkdirSync(dirname(join(dir, name)), { recursive: true });
  writeFileSync(join(dir, name), text);
};
let failed = 0;
const check = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n  ' + detail : ''}`);
  }
};

write('main.js', "import hello from 'virtual:hello';\nimport { tag } from './tag.js';\nimport again from 'virtual:again';\nimport { fromConfig } from './config-target.js';\nconsole.log(hello, tag, again, fromConfig, __WHO__);\n");
write('tag.js', "export const tag = 'ORIGINAL';\n");
write('config-target.js', "export const fromConfig = 'config-original';\n");
write('replace-plugin.mjs', "export default (options) => ({ name: 'replace-module', transform(code) { return code.replaceAll(options.from, options.to); } });\n");
// A config file the build would have used anyway.
write('rathkit.config.mjs', "export default { plugins: [{ name: 'from-config', transform(code, id) { if (id.endsWith('config-target.js')) return code.replace('config-original', 'config-applied'); } }] };\n");

const events = [];
const state = { transforms: 0 };
const first = {
  name: 'objects',
  buildStart() { events.push('buildStart'); },
  resolveId(source) { if (source === 'virtual:hello') return '\0virtual:hello'; },
  load(id) { if (id === '\0virtual:hello') return 'export default "virtual says hi";'; },
  transform(code, id) {
    state.transforms++;
    if (id.endsWith('tag.js')) return { code: code.replace('ORIGINAL', 'TRANSFORMED'), map: null };
  },
  buildEnd() { events.push('buildEnd'); },
  renderChunk(code) { return '/* banner by plugin */\n' + code; },
  generateBundle(options, bundle) {
    events.push('generateBundle:' + Object.keys(bundle).join(','));
    for (const file of Object.values(bundle)) if (file.type === 'chunk') file.code += '\n//generated';
  },
  closeBundle() { events.push('closeBundle'); },
};
const second = {
  name: 'context',
  async resolveId(source, importer) {
    if (source !== 'virtual:again') return null;
    // Resolving through the build, from here.
    const found = await this.resolve('./tag.js', importer);
    this.warn('careful, ' + (found ? 'resolved' : 'unresolved'));
    return { id: '\0virtual:again', moduleSideEffects: true };
  },
  load(id) { if (id === '\0virtual:again') return 'export default "again";'; },
};

try {
  const out = join(dir, 'out.mjs');
  const result = await build({
    cwd: dir,
    entries: ['main.js'],
    outfile: 'out.mjs',
    format: 'esm',
    define: { __WHO__: '"defined"' },
    plugins: [first, [second, { module: './replace-plugin.mjs', options: { from: 'virtual says hi', to: 'module replaced' } }]],
  });
  const text = readFileSync(out, 'utf8');
  check(text.includes('/* banner by plugin */') && text.includes('//generated'), 'renderChunk and generateBundle changes are in the output', text.slice(0, 200));
  const run = spawnSync('node', [out], { encoding: 'utf8' });
  check(run.stdout.trim() === 'module replaced TRANSFORMED again config-applied defined', 'virtual modules, transform, a module plugin and the config file all applied', run.stdout + run.stderr);
  check(events.join(' ') === 'buildStart buildEnd generateBundle:main.js closeBundle' || (events[0] === 'buildStart' && events.includes('closeBundle')), 'hooks ran in this process, in order', events.join(' '));
  check(state.transforms > 0, 'and their state is kept here');
  check(result.warnings.some((w) => w.includes('careful, resolved')), 'this.resolve answered and this.warn reached the build', JSON.stringify(result.warnings));

  // A hook that throws fails the build with its message.
  let failure = '';
  await build({ cwd: dir, entries: ['main.js'], outfile: 'x.mjs', plugins: [{ name: 'boom', transform() { throw new Error('boom from a hook'); } }] }).catch((e) => (failure = e.message + '\n' + e.stderr));
  check(/boom from a hook/.test(failure), 'an error in a hook is the build\'s error', failure);

  // The context methods that answer at once: the hook waits for the build.
  {
    const seen = {};
    const blocking = {
      name: 'blocking',
      buildStart() {
        seen.ref = this.emitFile({ type: 'asset', fileName: 'emitted.txt', source: 'from a plugin object' });
        seen.name = this.getFileName(seen.ref);
        seen.ast = this.parse('const a = 1;').body[0].type;
      },
      buildEnd() {
        const info = this.getModuleInfo(join(dir, 'main.js'));
        seen.info = Boolean(info) && info.id === join(dir, 'main.js');
        seen.ids = [...this.getModuleIds()].length > 0;
      },
    };
    await build({ cwd: dir, entries: ['main.js'], outfile: 'blocking.mjs', format: 'esm', define: { __WHO__: '"d"' }, plugins: [first, second, blocking] });
    check(seen.name === 'emitted.txt' && typeof seen.ref === 'string', 'this.emitFile and this.getFileName answer at once', JSON.stringify(seen));
    check(readFileSync(join(dir, 'emitted.txt'), 'utf8') === 'from a plugin object', 'and what is emitted is written');
    check(seen.ast === 'VariableDeclaration', 'this.parse returns the tree', JSON.stringify(seen));
    check(seen.info === true && seen.ids === true, 'this.getModuleInfo and this.getModuleIds answer', JSON.stringify(seen));
  }

  // What needs the build's own objects says so.
  failure = '';
  await build({ cwd: dir, entries: ['main.js'], outfile: 'x.mjs', plugins: [{ name: 'emitter', buildStart() { this.addWatchFiles; this.getCombinedSourcemap(); } }] }).catch((e) => (failure = e.message + '\n' + e.stderr));
  check(/getCombinedSourcemap is not available to a plugin object/.test(failure), 'a method that cannot cross says it is not available', failure);
  failure = '';
  await build({ cwd: dir, entries: ['main.js'], outfile: 'x.mjs', plugins: [{ name: 'server', configurePreviewServer() {} }] }).catch((e) => (failure = e.message + '\n' + e.stderr));
  check(/configurePreviewServer/.test(failure) && /config file/.test(failure), 'a hook that needs the build\'s own objects is refused with a pointer to config files', failure);

  // configureServer: middlewares (the request and its body go across; next() passes it on) and ws.send.
  {
    const dev = {
      name: 'dev',
      configureServer(server) {
        server.middlewares.use('/api', (req, res, next) => {
          if (req.url === '/hello') {
            res.setHeader('content-type', 'text/plain');
            res.end('hello ' + req.method + ' ' + server.config.mode);
          } else if (req.url === '/ping') {
            server.ws.send('ping', { n: 1 });
            res.end('sent');
          } else if (req.url === '/echo') {
            let body = '';
            req.on('data', (d) => (body += d));
            req.on('end', () => res.end(JSON.stringify({ body, type: req.headers['content-type'] })));
          } else next();
        });
        return () => {};
      },
    };
    const live = await serve({ cwd: dir, entries: ['main.js'], port: 0, plugins: [dev] });
    const get = (path, init) => fetch(new URL(path, live.url), init).then(async (r) => ({ status: r.status, text: await r.text() }));
    const hello = await get('api/hello');
    check(hello.status === 200 && hello.text === 'hello GET development', 'a middleware of a plugin object answers', JSON.stringify(hello));
    const echo = await get('api/echo', { method: 'POST', body: 'posted', headers: { 'content-type': 'text/x-test' } });
    check(echo.text === '{"body":"posted","type":"text/x-test"}', 'and sees the body of a request', echo.text);
    const other = await get('api/other');
    check(other.status === 404, 'next() passes a request on', JSON.stringify(other));
    const messages = [];
    const controller = new AbortController();
    fetch(new URL('__rathkit/events', live.url), { signal: controller.signal }).then(async (response) => {
      const reader = response.body.getReader();
      let buffer = '';
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        buffer += Buffer.from(value).toString();
        for (let at; (at = buffer.indexOf('\n\n')) >= 0; buffer = buffer.slice(at + 2)) {
          try {
            messages.push(JSON.parse(buffer.slice(0, at).replace(/^data: /, '')));
          } catch {}
        }
      }
    }).catch(() => {});
    await new Promise((r) => setTimeout(r, 300));
    await get('api/ping');
    let found = false;
    for (let tries = 0; tries < 50 && !found; tries++) {
      found = messages.some((m) => m.type === 'custom' && m.event === 'ping' && m.data.n === 1);
      if (!found) await new Promise((r) => setTimeout(r, 100));
    }
    check(found, 'server.ws.send reaches the page', JSON.stringify(messages));
    controller.abort();
    await live.close();
  }

  // And the development server takes them too.
  const server = await serve({ cwd: dir, entries: ['main.js'], port: 0, plugins: [first, second] });
  let bundle = '';
  for (let tries = 0; tries < 200 && !bundle.includes('virtual says hi'); tries++) {
    bundle = await fetch(new URL('bundle.js', server.url)).then((r) => (r.ok ? r.text() : ''), () => '');
    if (!bundle) await new Promise((r) => setTimeout(r, 100));
  }
  check(bundle.includes('virtual says hi') && bundle.includes('TRANSFORMED'), 'serve runs plugin objects', bundle.slice(0, 200));
  await server.close();
} catch (error) {
  failed++;
  console.log('FAIL ' + (error?.stack ?? error) + (error?.stderr ? '\n' + error.stderr : ''));
} finally {
  rmSync(dir, { recursive: true, force: true });
}
if (failed) {
  console.log(`${failed} failure(s)`);
  process.exit(1);
}
console.log('plugin objects: all checks pass');
