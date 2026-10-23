// Module federation in a real browser: applications built by rathkit-pack and by rspack load each other's modules and
// agree on one copy of React, and two development servers do the same with a change in one reloading the other's page.
//   node tools/run-federation.mjs [scenario...]
// Needs a Chromium-based browser (CHROME, a Playwright download, or one on the PATH). The scenarios that involve rspack need
// `npm ci --prefix bench`. Without a browser it says so and passes.
import http from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdtempSync, rmSync, cpSync, symlinkSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir, homedir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rathkit-pack');
const tests = join(root, 'tests', 'federation');
const bench = join(root, 'bench');
const only = process.argv.slice(2);

const findBrowser = () => {
  if (process.env.CHROME) return process.env.CHROME;
  const cache = join(homedir(), '.cache', 'ms-playwright');
  if (existsSync(cache)) {
    for (const dir of readdirSync(cache).filter((d) => d.startsWith('chromium_headless_shell')).sort().reverse()) {
      const found = join(cache, dir, 'chrome-headless-shell-linux64', 'chrome-headless-shell');
      if (existsSync(found)) return found;
    }
    for (const dir of readdirSync(cache).filter((d) => d.startsWith('chromium-')).sort().reverse()) {
      const found = join(cache, dir, 'chrome-linux64', 'chrome');
      if (existsSync(found)) return found;
    }
  }
  for (const name of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'chrome']) {
    if (spawnSync('which', [name], { encoding: 'utf8' }).status === 0) return name;
  }
  return null;
};
const browser = findBrowser();
if (!browser) {
  console.log('federation: no Chromium-based browser found (set CHROME); skipped');
  process.exit(0);
}

const tmp = mkdtempSync(join(tmpdir(), 'federation-'));
const types = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json', '.html': 'text/html', '.css': 'text/css' };
const servers = [];
const serve = (port, dir) => {
  const server = http.createServer((req, res) => {
    const path = join(dir, decodeURIComponent(req.url.split('?')[0]).replace(/\/$/, '/index.html'));
    if (!existsSync(path)) {
      res.writeHead(404, { 'access-control-allow-origin': '*' });
      res.end('missing');
      return;
    }
    res.writeHead(200, { 'content-type': types[extname(path)] ?? 'application/octet-stream', 'access-control-allow-origin': '*', 'cache-control': 'no-store' });
    res.end(readFileSync(path));
  });
  server.listen(port);
  servers.push(server);
};

// Builds a project of tests/federation with rathkit-pack into dist/ of a copy, so the sources stay clean.
const copies = new Map();
const copy = (name) => {
  if (!copies.has(name)) {
    const dir = join(tmp, name);
    cpSync(join(tests, name), dir, { recursive: true });
    // The packages the projects import are the ones in the bench folder.
    symlinkSync(join(bench, 'node_modules'), join(dir, 'node_modules'), 'dir');
    copies.set(name, dir);
  }
  return copies.get(name);
};
const build = async (name, { entry = 'src/index.js', args = [], patch } = {}) => {
  const dir = copy(name);
  if (patch) patch(dir);
  // The options are in `mf.json`, or in the config file as a plugin.
  const options = existsSync(join(dir, 'mf.json')) ? ['--federation', 'mf.json'] : [];
  // Not spawnSync: the servers of the test run in this process, and a build may fetch from them.
  const r = await new Promise((resolve) => {
    const child = spawn(bin, ['bundle', entry, ...options, '--outdir', 'dist', '--define', 'process.env.NODE_ENV="production"', ...args], { cwd: dir });
    let stderr = '';
    child.stderr.on('data', (d) => (stderr += d));
    child.on('close', (status) => resolve({ status, stderr }));
  });
  if (r.status !== 0) throw new Error(`building ${name}: ${r.stderr}`);
  if (existsSync(join(dir, 'index.html'))) cpSync(join(dir, 'index.html'), join(dir, 'dist', 'index.html'));
  return join(dir, 'dist');
};
// A test project's options, with the address of the remote it talks to changed.
const withOption = (file, change) => (dir) => {
  const path = join(dir, file);
  const json = JSON.parse(readFileSync(path, 'utf8'));
  change(json);
  writeFileSync(path, JSON.stringify(json));
};

let rspack;
const buildRspack = async (name) => {
  const dir = copy(name);
  rspack ??= await import(join(bench, 'node_modules', '@rspack', 'core', 'dist', 'index.js')).catch(() => null);
  if (!rspack) throw new Error('rspack is not installed (npm ci --prefix bench)');
  const config = (await import(join(dir, 'rspack.config.mjs'))).default;
  // Resolve the packages from the bench folder.
  config.resolve = { modules: [join(bench, 'node_modules'), 'node_modules'] };
  config.resolveLoader = { modules: [join(bench, 'node_modules')] };
  await new Promise((resolve, reject) => rspack.rspack(config, (error, stats) => (error ? reject(error) : stats.hasErrors() ? reject(new Error(stats.toString({ errors: true }))) : resolve())));
  return join(dir, 'dist');
};
const haveRspack = existsSync(join(bench, 'node_modules', '@module-federation', 'enhanced')) && existsSync(join(bench, 'node_modules', '@rspack', 'core'));

// A page driven through the browser's debugging protocol.
let browserCount = 0;
const openPage = async (url, { wait = 3000 } = {}) => {
  const port = 9400 + (browserCount++ % 100);
  const profile = mkdtempSync(join(tmp, 'profile-'));
  const child = spawn(browser, ['--no-sandbox', '--disable-gpu', '--headless=new', `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, 'about:blank'], { stdio: 'ignore' });
  let targets;
  for (let i = 0; i < 50 && !targets; i++) {
    await new Promise((r) => setTimeout(r, 200));
    targets = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json()).catch(() => undefined);
  }
  if (!targets) throw new Error('the browser did not start');
  const ws = new WebSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  let id = 0;
  const pending = new Map();
  const logs = [];
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) {
      pending.get(d.id)(d.result);
      pending.delete(d.id);
    } else if (d.method === 'Runtime.consoleAPICalled') logs.push(d.params.args.map((a) => a.value ?? a.description).join(' '));
    else if (d.method === 'Runtime.exceptionThrown') logs.push('EXCEPTION ' + (d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text));
  };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Page.navigate', { url });
  const page = {
    logs,
    eval: async (expression) => (await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result?.value,
    // The text of the page and its title, once its script has finished (or after `wait` ms).
    async settled(timeout = wait) {
      const started = Date.now();
      while (Date.now() - started < timeout) {
        const title = await page.eval('document.title');
        if (title === 'done' || title === 'failed') break;
        await new Promise((r) => setTimeout(r, 100));
      }
      return { title: await page.eval('document.title'), text: await page.eval("document.getElementById('out')?.textContent") };
    },
    close() {
      ws.close();
      child.kill();
    },
  };
  return page;
};

let failed = 0;
const scenarios = [];
const scenario = (name, fn) => scenarios.push([name, fn]);
const check = (name, condition, detail) => {
  if (!condition) {
    failed++;
    console.log(`FAIL ${name}${detail ? `\n  ${detail}` : ''}`);
  }
};
const visit = async (url) => {
  const page = await openPage(url);
  try {
    const result = await page.settled(8000);
    return { ...result, logs: page.logs };
  } finally {
    page.close();
  }
};

scenario('a host and a remote built by rathkit-pack', async () => {
  const remote = await build('remote');
  const host = await build('host');
  serve(4210, remote);
  serve(4211, host);
  const r = await visit('http://localhost:4211/');
  check('page finished', r.title === 'done', JSON.stringify(r));
  check('static import of the remote works', /button 19\.3\.1/.test(r.text ?? ''), r.text);
  check('the remote shares the host\'s React', /same=true/.test(r.text ?? '') && !/same=false/.test(r.text ?? ''), r.text);
  check('import() of the remote works', /dyn:button/.test(r.text ?? ''), r.text);
  check('no errors', !r.logs.some((l) => /EXCEPTION|Error/.test(l)), r.logs.join('\n'));
  // The manifest others read.
  const manifest = JSON.parse(readFileSync(join(remote, 'mf-manifest.json'), 'utf8'));
  check('manifest names the remote', manifest.name === 'oapp' && manifest.metaData.remoteEntry.name === 'remoteEntry.js', JSON.stringify(manifest.metaData));
  check('manifest lists the expose and the shared package', manifest.exposes[0].name === 'Button' && manifest.shared[0].name === 'react' && manifest.shared[0].singleton === true, JSON.stringify(manifest));
});

if (haveRspack) {
  scenario('a host built by rathkit-pack loads a remote built by rspack', async () => {
    const remote = await buildRspack('rspack-remote');
    const host = await build('host', {
      patch: (dir) => {
        withOption('mf.json', (j) => (j.remotes = { oapp: 'rapp@http://localhost:4220/remoteEntry.js' }))(dir);
      },
    });
    serve(4220, remote);
    serve(4221, host);
    const r = await visit('http://localhost:4221/');
    check('page finished', r.title === 'done', JSON.stringify(r));
    check('the rspack remote\'s module runs', /button 19\.3\.1/.test(r.text ?? ''), r.text);
    check('and uses the host\'s React', /same=true/.test(r.text ?? '') && !/same=false/.test(r.text ?? ''), r.text);
  });

  scenario('a host built by rspack loads a remote built by rathkit-pack', async () => {
    const remote = await build('remote');
    const host = await buildRspack('rspack-host');
    cpSync(join(copy('host'), 'index.html'), join(host, 'index.html'));
    writeFileSync(join(host, 'index.html'), readFileSync(join(host, 'index.html'), 'utf8').replace('<script type="module" src="/index.js"></script>', '<script src="/main.js"></script>'));
    serve(4210, remote);
    serve(4231, host);
    const r = await visit('http://localhost:4231/');
    check('page finished', r.title === 'done', JSON.stringify(r));
    check('our module runs in their host', /button 19\.3\.1/.test(r.text ?? '') && /remote-label/.test(r.text ?? ''), r.text);
    check('with their React', /same=true/.test(r.text ?? '') && !/same=false/.test(r.text ?? ''), r.text);
  });
} else {
  console.log('federation: rspack is not installed (npm ci --prefix bench); the scenarios with it are skipped');
}

scenario('an ES module container, prefix and eager shared packages, runtime plugins', async () => {
  const remote = await build('remote-module', { args: ['--minify'] });
  const host = await build('host-options', { args: ['--minify'] });
  serve(4250, remote);
  serve(4251, host);
  check('the manifest has the name asked for', existsSync(join(remote, 'federation-manifest.json')));
  check('the container is an ES module', /^const base/.test(readFileSync(join(remote, 'remoteEntry.js'), 'utf8')) && /export const get/.test(readFileSync(join(remote, 'remoteEntry.js'), 'utf8')));
  const r = await visit('http://localhost:4251/');
  check('page finished', r.title === 'done', JSON.stringify(r));
  const text = r.text ?? '';
  check('a deep import of a shared package works in both', /map 2,4/.test(text) && /doubled 6/.test(text), text);
  check('named and default imports from a remote', /shout Hi!/.test(text) && /default utils-default/.test(text), text);
  check('plugin: a request is renamed, a lost remote answered, loads seen', /old A!/.test(text) && /gone fallback for r2\/Gone from params/.test(text) && /loaded r2\/utils/.test(text), text);
  check('one copy of lodash-es is in the share scope', /lodash copies 1/.test(text), text);
  check('an eager shared package', /react 19\.3\.1/.test(text), text);
  check('no errors', !r.logs.some((l) => /EXCEPTION/.test(l)), r.logs.join('\n'));
});

scenario('types: a remote publishes declarations and a host checks its imports against them', async () => {
  const typescript = join(bench, 'node_modules', 'typescript', 'bin', 'tsc');
  if (!existsSync(typescript)) {
    console.log('federation: typescript is not installed (npm ci --prefix bench); the types scenario is skipped');
    return;
  }
  const remote = await build('remote-types', { entry: 'src/index.ts' });
  for (const file of ['@mf-types.zip', '@mf-types.d.ts', '@mf-types/apis.d.ts', '@mf-types/compiled-types/Button.d.ts']) check(`the remote publishes ${file}`, existsSync(join(remote, file)));
  const manifest = JSON.parse(readFileSync(join(remote, 'mf-manifest.json'), 'utf8'));
  check('the manifest points to the zip', manifest.metaData.types.zip === '@mf-types.zip' && manifest.metaData.types.api === '@mf-types.d.ts', JSON.stringify(manifest.metaData.types));
  check('the declarations describe the module', /export default function makeButton\(props: ButtonProps\): string/.test(readFileSync(join(remote, '@mf-types', 'compiled-types', 'Button.d.ts'), 'utf8')));
  serve(4260, remote);
  const hostDir = copy('host-types');
  const host = await build('host-types', { entry: 'src/index.ts' });
  check('the host reads them', existsSync(join(hostDir, '@mf-types', 'r3', 'compiled-types', 'Button.d.ts')));
  check('and maps the module to them in tsconfig.json', JSON.parse(readFileSync(join(hostDir, 'tsconfig.json'), 'utf8')).compilerOptions.paths?.['r3/Button']?.[0] === './@mf-types/r3/compiled-types/Button');
  const check_types = () => spawnSync('node', [typescript, '-p', hostDir], { encoding: 'utf8' });
  const good = check_types();
  check('code that uses the remote correctly passes the type check', good.status === 0, good.stdout);
  const source = join(hostDir, 'src', 'index.ts');
  writeFileSync(source, readFileSync(source, 'utf8').replace("size: version", "size: 'big'"));
  const bad = check_types();
  check('code that uses it wrongly fails it', bad.status !== 0 && /Type 'string' is not assignable to type 'number'/.test(bad.stdout), bad.stdout);
  serve(4261, host);
  const r = await visit('http://localhost:4261/');
  check('and runs', r.title === 'done' && /typed:3/.test(r.text ?? ''), JSON.stringify(r));
});

scenario('development servers: a page of one loads the other, and a change in the remote reloads it', async () => {
  const remoteDir = copy('remote');
  const hostDir = copy('host');
  withOption('mf.json', (j) => (j.remotes = { oapp: 'oapp@http://localhost:4240/remoteEntry.js' }))(hostDir);
  const procs = [];
  const start = (dir, port) => {
    const p = spawn(bin, ['serve', 'src/index.js', '--federation', 'mf.json', '--port', String(port), '--define', 'process.env.NODE_ENV="production"'], { cwd: dir, stdio: 'ignore' });
    procs.push(p);
  };
  try {
    start(remoteDir, 4240);
    start(hostDir, 4241);
    for (let i = 0; i < 50; i++) {
      const up = await fetch('http://localhost:4241/index.js').then((r) => r.ok, () => false);
      if (up && (await fetch('http://localhost:4240/remoteEntry.js').then((r) => r.ok, () => false))) break;
      await new Promise((r) => setTimeout(r, 100));
    }
    const page = await openPage('http://localhost:4241/');
    try {
      const first = await page.settled(8000);
      check('the host page loads the remote from the other server', first.title === 'done' && /remote-label/.test(first.text ?? '') && /same=true/.test(first.text ?? ''), JSON.stringify(first));
      const file = join(remoteDir, 'src', 'Button.js');
      writeFileSync(file, readFileSync(file, 'utf8').replace('remote-label', 'remote-label-CHANGED'));
      let after;
      for (let i = 0; i < 60; i++) {
        await new Promise((r) => setTimeout(r, 100));
        after = await page.eval("document.getElementById('out')?.textContent");
        if (/CHANGED/.test(after ?? '')) break;
      }
      check('a change in the remote reloads the page of the host', /remote-label-CHANGED/.test(after ?? ''), after);
    } finally {
      page.close();
    }
  } finally {
    for (const p of procs) p.kill();
  }
});

for (const [name, fn] of scenarios) {
  if (only.length && !only.some((o) => name.includes(o))) continue;
  try {
    await fn();
  } catch (error) {
    failed++;
    console.log(`FAIL ${name}: ${error.stack ?? error}`);
  }
  // The next scenario uses the same ports.
  for (const s of servers.splice(0)) {
    s.closeAllConnections();
    await new Promise((r) => s.close(r));
  }
}
rmSync(tmp, { recursive: true, force: true, maxRetries: 5 });
console.log(failed ? `${failed} failure(s)` : `all ${scenarios.length} federation scenarios agree`);
process.exit(failed ? 1 : 0);
