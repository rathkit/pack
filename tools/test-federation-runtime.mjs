// Tests of the federation runtime (src/federation/runtime.js) without a browser, with remotes that are made up
// in the test: how modules are found, which copy of a shared package is picked, and what plugins can change.
//   node tools/test-federation-runtime.mjs
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const rt = await import(join(root, 'src', 'federation', 'runtime.js'));

let count = 0;
const tests = [];
const test = (name, fn) => tests.push([name, fn]);
let serial = 0;
const unique = (name) => `${name}${++serial}`;

// A container as another application would offer it.
const container = ({ shared = {}, exposes = {}, name = 'remote' } = {}) => {
  const calls = { init: [], get: [] };
  return {
    calls,
    async init(scope, initScope, options) {
      calls.init.push({ scope, options });
      for (const [pkg, { version, from = name, loaded, lib, eager }] of Object.entries(shared)) {
        scope[pkg] ||= {};
        if (!scope[pkg][version]) scope[pkg][version] = { version, from, scope: ['default'], shareConfig: { singleton: true, requiredVersion: `^${version}`, eager: !!eager }, get: () => Promise.resolve(() => lib ?? { from, version }), loaded, lib: loaded ? () => lib : undefined };
      }
    },
    async get(expose) {
      calls.get.push(expose);
      if (!(expose in exposes)) throw new Error(`Module "${expose}" does not exist in container.`);
      return () => exposes[expose];
    },
  };
};

// A plugin that hands out containers instead of loading scripts.
const entries = (map) => ({
  name: unique('entries'),
  loadEntry: ({ remoteInfo }) => map[remoteInfo.name],
});

const host = (options, containers = {}) => rt.createInstance({ name: unique('host'), ...options, plugins: [entries(containers), ...(options.plugins || [])] });

test('a remote module is found by its name and expose', async () => {
  const c = container({ exposes: { './Button': { default: () => 'btn', label: 'L' } } });
  const h = host({ remotes: [{ name: 'app', entry: 'http://x/app.js' }] }, { app: c });
  const m = await h.loadRemote('app/Button');
  assert.equal(m.default(), 'btn');
  assert.equal(m.label, 'L');
  assert.deepEqual(c.calls.get, ['./Button']);
  await h.loadRemote('app/Button');
  assert.equal(c.calls.init.length, 1, 'the container starts once');
});

test('the root of a remote is its "." expose', async () => {
  const c = container({ exposes: { '.': { v: 1 } } });
  const h = host({ remotes: [{ name: 'app', entry: 'x' }] }, { app: c });
  assert.deepEqual(await h.loadRemote('app'), { v: 1 });
});

test('the longest remote name wins, and an alias stands for a name', async () => {
  const a = container({ exposes: { './x': 'in a' } });
  const ab = container({ exposes: { './x': 'in a/b' } });
  const h = host({ remotes: [{ name: 'a', entry: 'x' }, { name: 'a/b', entry: 'x' }, { name: 'long', alias: 'short', entry: 'x' }] }, { a, 'a/b': ab, long: container({ exposes: { './y': 'aliased' } }) });
  assert.equal(await h.loadRemote('a/b/x'), 'in a/b');
  assert.equal(await h.loadRemote('short/y'), 'aliased');
  assert.equal(await h.loadRemote('long/y'), 'aliased');
});

test('an unknown remote or expose fails with a message that says which', async () => {
  const h = host({ remotes: [{ name: 'app', entry: 'x' }] }, { app: container({ exposes: {} }) });
  await assert.rejects(h.loadRemote('nope/x'), /No remote matches "nope\/x"/);
  await assert.rejects(h.loadRemote('app/Missing'), /does not exist in container/);
});

test('the container gets the share scope of the host', async () => {
  const c = container({ shared: { lib: { version: '1.2.0' } }, exposes: { './x': 1 } });
  const h = host({ remotes: [{ name: 'app', entry: 'x' }], shared: { lib: { version: '1.3.0', lib: () => ({ from: 'host' }) } } }, { app: c });
  await h.loadRemote('app/x');
  const versions = Object.keys(h.shareScopeMap.default.lib).sort();
  assert.deepEqual(versions, ['1.2.0', '1.3.0'], 'both are offered');
  assert.equal(c.calls.init[0].scope, h.shareScopeMap.default);
  assert.equal(c.calls.init[0].options.shareScopeMap, h.shareScopeMap);
});

test('a singleton is the newest version offered, whoever offers it', async () => {
  const c = container({ shared: { lib: { version: '2.1.0', lib: { from: 'remote' } } } });
  const h = host({ remotes: [{ name: 'app', entry: 'x' }], shared: { lib: { version: '2.0.0', scope: 'default', shareConfig: { singleton: true, requiredVersion: '^2.0.0' }, get: () => Promise.resolve(() => ({ from: 'host' })) } } }, { app: c });
  const lib = await h.loadShare('lib');
  assert.equal(lib().from, 'remote');
});

test('a version that does not satisfy a non-singleton need is not used', async () => {
  const c = container({ shared: { lib: { version: '3.0.0', lib: { from: 'remote' } } } });
  const h = host({ remotes: [{ name: 'app', entry: 'x' }], shared: { lib: { version: '2.0.0', scope: 'default', shareConfig: { singleton: false, requiredVersion: '^2.0.0' }, get: () => Promise.resolve(() => ({ from: 'host' })) } } }, { app: c });
  assert.equal((await h.loadShare('lib'))().from, 'host');
});

test('strictVersion makes a singleton that does not satisfy an error, and otherwise it warns', async () => {
  const mk = (strictVersion) => {
    const c = container({ shared: { lib: { version: '3.0.0', lib: { from: 'remote' } } } });
    return host({ remotes: [{ name: 'app', entry: 'x' }], shared: { lib: { version: '2.0.0', scope: 'default', shareConfig: { singleton: true, strictVersion, requiredVersion: '^2.0.0' }, get: () => Promise.resolve(() => ({ from: 'host' })) } } }, { app: c });
  };
  const warnings = [];
  const warn = console.warn;
  console.warn = (m) => warnings.push(m);
  try {
    assert.equal((await mk(false).loadShare('lib'))().from, 'remote');
    assert.ok(warnings.some((w) => /does not satisfy/.test(w)));
    await assert.rejects(mk(true).loadShare('lib'), /does not satisfy/);
  } finally {
    console.warn = warn;
  }
});

test('a package nobody else offers comes from the host, once', async () => {
  let loads = 0;
  const h = host({ shared: { lib: { version: '1.0.0', scope: 'default', shareConfig: { singleton: true }, get: () => { loads++; const value = { n: loads }; return Promise.resolve(() => value); } } } });
  const [a, b] = await Promise.all([h.loadShare('lib'), h.loadShare('lib')]);
  assert.equal(a().n, 1);
  assert.equal(b(), a());
  assert.equal(loads, 1);
});

test('a package that is only consumed fails when nobody provides it', async () => {
  const h = host({ shared: { lib: { version: '1.0.0', scope: 'default', consumeOnly: true, shareConfig: { singleton: true }, get: () => Promise.reject(new Error('no copy')) } } });
  await assert.rejects(h.loadShare('lib'), /not bundled/);
});

test('loaded-first prefers a copy that has loaded over a newer one', async () => {
  const h = host({ shareStrategy: 'loaded-first', shared: {} });
  h.shareScopeMap.default = {
    lib: {
      '1.0.0': { version: '1.0.0', from: 'a', scope: ['default'], loaded: true, lib: () => 'old', get: () => Promise.resolve(() => 'old'), shareConfig: { singleton: true }, strategy: 'loaded-first', useIn: [] },
      '2.0.0': { version: '2.0.0', from: 'b', scope: ['default'], get: () => Promise.resolve(() => 'new'), shareConfig: { singleton: true }, strategy: 'loaded-first', useIn: [] },
    },
  };
  const info = { version: '1.0.0', from: 'h', scope: ['default'], strategy: 'loaded-first', shareConfig: { singleton: true, requiredVersion: '^1.0.0' }, get: () => Promise.resolve(() => 'own'), useIn: [] };
  h.options.shared.lib = [info];
  assert.equal((await h.loadShare('lib'))(), 'old');
});

test('plugins can change a request, take over a failure and watch loads', async () => {
  const seen = [];
  const plugin = {
    name: unique('p'),
    beforeRequest(args) {
      seen.push('before ' + args.id);
      if (args.id === 'app/Old') args.id = 'app/New';
      return args;
    },
    afterResolve(args) {
      seen.push('resolve ' + args.expose);
      return args;
    },
    onLoad(args) {
      seen.push('load ' + args.id);
    },
    errorLoadRemote(args) {
      seen.push('error ' + args.id);
      if (args.id === 'app/Gone') return { default: 'fallback' };
    },
  };
  const h = host({ remotes: [{ name: 'app', entry: 'x' }], plugins: [plugin] }, { app: container({ exposes: { './New': { v: 'new' } } }) });
  assert.deepEqual(await h.loadRemote('app/Old'), { v: 'new' });
  assert.deepEqual(await h.loadRemote('app/Gone'), { default: 'fallback' });
  await assert.rejects(h.loadRemote('app/Absent'), /does not exist/);
  assert.deepEqual(seen.slice(0, 4), ['before app/Old', 'resolve ./New', 'load app/New', 'before app/Gone']);
  assert.ok(seen.includes('error app/Gone') && seen.includes('error app/Absent'));
});

test('registering a remote again keeps the first unless forced', async () => {
  const h = host({ remotes: [{ name: 'app', entry: 'one' }] }, { app: container({ exposes: { './x': 'one' } }) });
  const warn = console.warn;
  console.warn = () => {};
  h.registerRemotes([{ name: 'app', entry: 'two' }]);
  console.warn = warn;
  assert.equal(h.options.remotes[0].entry, 'one');
  h.registerRemotes([{ name: 'app', entry: 'two' }], { force: true });
  assert.equal(h.options.remotes[0].entry, 'two');
});

test('a container serves its exposes after its shared packages are chosen', async () => {
  const order = [];
  const h = rt.createInstance({ name: unique('app'), shared: { lib: { version: '1.0.0', scope: 'default', shareConfig: { singleton: true }, get: () => Promise.resolve(() => 'own') } } });
  const c = rt.containerFor(h, {
    exposes: { './x': async () => (order.push('exposed'), { default: 'x' }) },
    ready: async () => void order.push('ready'),
    shareScope: ['default'],
  });
  const scope = { lib: { '2.0.0': { version: '2.0.0', from: 'host', scope: ['default'], shareConfig: { singleton: true }, get: () => Promise.resolve(() => 'theirs') } } };
  await c.init(scope, [], { shareScopeKeys: 'default' });
  assert.deepEqual(Object.keys(scope.lib).sort(), ['1.0.0', '2.0.0'], 'it offers its own beside the host\'s');
  const factory = await c.get('./x');
  assert.deepEqual(order, ['ready', 'exposed']);
  const m = factory();
  assert.equal(m.default, 'x');
  assert.equal(m.__esModule, undefined, 'a plain object is left as it is');
  await assert.rejects(c.get('./nope'), /does not exist in container/);
});

test('an ES module namespace leaves a container flagged as one, as other bundlers expect', async () => {
  const ns = await import('data:text/javascript,export default function f(){return 1};export const a=2');
  const h = rt.createInstance({ name: unique('app') });
  const factory = await rt.containerFor(h, { exposes: { './m': () => ns } }).get('./m');
  const m = factory();
  assert.equal(m.__esModule, true);
  assert.equal(m.default(), 1);
  assert.equal(m.a, 2);
  assert.deepEqual(Object.keys(m).sort(), ['a', 'default']);
});

test('values read the way import and require read them', async () => {
  const ns = await import('data:text/javascript,export default 7;export const n=1');
  const view = rt.moduleValue(ns);
  assert.equal(view.__esModule, true);
  assert.equal(view.default, 7);
  assert.equal(rt.moduleValue({ x: 1 }).x, 1);
  const cjs = Object.assign(Object.create(null, { [Symbol.toStringTag]: { value: 'Module' } }), { a: 1, b: 2 });
  const exports = { a: 1, b: 2 };
  Object.defineProperty(cjs, 'default', { value: exports, enumerable: true });
  const mirrored = Object.assign(Object.create(null, { [Symbol.toStringTag]: { value: 'Module' } }), { a: exports.a, b: exports.b, default: exports });
  assert.equal(rt.fallbackValue(mirrored), exports, 'a CommonJS package is read as its exports');
  assert.equal(rt.fallbackValue(ns).default, 7, 'an ES module stays one');
  assert.equal(rt.moduleNamespace({ a: 1 }).default.a, 1);
});

test('the official runtime package names are this runtime', async () => {
  assert.equal(typeof rt.init, 'function');
  for (const name of ['loadRemote', 'loadShare', 'loadShareSync', 'preloadRemote', 'registerRemotes', 'registerPlugins', 'registerShared', 'getInstance', 'createInstance']) assert.equal(typeof rt[name], 'function', name);
});

for (const [name, fn] of tests) {
  try {
    await fn();
    count++;
  } catch (error) {
    console.log(`FAIL ${name}\n  ${error.stack.split('\n').slice(0, 4).join('\n  ')}`);
    process.exitCode = 1;
  }
}
console.log(process.exitCode ? `${tests.length - count} of ${tests.length} failed` : `all ${count} federation runtime tests pass`);
