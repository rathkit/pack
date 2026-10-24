// The module federation runtime: an application finds other applications' modules (remotes) and
// agrees with them on one copy of each shared package. It speaks the container protocol that
// webpack, rspack and the Vite plugins share, so a remote built by any of them can be loaded by
// this host, and the other way around:
//
//   container = { get(expose) -> Promise<() => module>, init(shareScope, initScope, options) }
//   shareScope = { [package]: { [version]: { get, from, loaded, scope, shareConfig, ... } } }
//
// Plugins are objects of hook functions, named as in the other runtimes (`beforeRequest`,
// `afterResolve`, `onLoad`, `errorLoadRemote`, `beforeLoadShare`, `resolveShare`, ...).

import { satisfy, lessThan } from './semver.js';

export { satisfy };

const DEFAULT_SCOPE = 'default';
const root = globalThis;
const registry = (root.__FEDERATION__ ||= {});
registry.__GLOBAL_PLUGIN__ ||= [];
registry.__INSTANCES__ ||= [];
registry.__SHARE__ ||= {};
registry.__MANIFEST_LOADING__ ||= {};
registry.__PRELOADED_MAP__ ||= new Map();
registry.moduleInfo ||= {};

const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
const isBrowser = () => typeof window !== 'undefined' && typeof document !== 'undefined';
const toArray = (v) => (Array.isArray(v) ? v : v === undefined || v === null ? [] : [v]);

function fail(code, message) {
  const error = new Error(`[Federation Runtime]: ${message} #${code}`);
  error.code = code;
  throw error;
}

function warn(message) {
  if (typeof console !== 'undefined') console.warn(`[Federation Runtime]: ${message instanceof Error ? message.message : message}`);
}

// ---- hooks ---------------------------------------------------------------------------------

// Runs every listener; the last value that is not `undefined` is the result.
class SyncHook {
  constructor(type = '') {
    this.type = type;
    this.listeners = new Set();
  }
  on(fn) {
    if (typeof fn === 'function') this.listeners.add(fn);
  }
  once(fn) {
    const wrapper = (...args) => {
      this.remove(wrapper);
      return fn(...args);
    };
    this.on(wrapper);
  }
  remove(fn) {
    this.listeners.delete(fn);
  }
  emit(...data) {
    let result;
    for (const fn of Array.from(this.listeners)) {
      const next = fn(...data);
      if (next !== undefined) result = next;
    }
    return result;
  }
}

// Runs the listeners one after another; one that returns `false` ends the chain, and the last
// value that is not `undefined` (or the argument itself) is the result.
class AsyncHook extends SyncHook {
  async emit(...data) {
    let result;
    for (const fn of Array.from(this.listeners)) {
      const next = await fn(...data);
      if (next === false) return false;
      if (next !== undefined && !(data.length === 1 && next === data[0])) result = next;
    }
    return result;
  }
}

const acceptable = (original, returned) => {
  if (!isObject(returned)) return false;
  if (original !== returned) {
    for (const key in original) if (!(key in returned)) return false;
  }
  return true;
};

// Each listener gets the data the one before it returned.
class SyncWaterfallHook extends SyncHook {
  emit(data) {
    if (!isObject(data)) fail('RUNTIME-000', `The data for the "${this.type}" hook should be an object.`);
    for (const fn of Array.from(this.listeners)) {
      try {
        const next = fn(data);
        if (next === undefined) continue;
        if (!acceptable(data, next)) fail('RUNTIME-000', `A plugin returned an unacceptable value for the "${this.type}" hook.`);
        data = next;
      } catch (error) {
        warn(error);
        throw error;
      }
    }
    return data;
  }
}

class AsyncWaterfallHook extends SyncHook {
  async emit(data) {
    if (!isObject(data)) fail('RUNTIME-000', `The data for the "${this.type}" hook should be an object.`);
    for (const fn of Array.from(this.listeners)) {
      try {
        const next = await fn(data);
        if (next === undefined) continue;
        if (!acceptable(data, next)) fail('RUNTIME-000', `A plugin returned an unacceptable value for the "${this.type}" hook.`);
        data = next;
      } catch (error) {
        warn(error);
        throw error;
      }
    }
    return data;
  }
}

class PluginSystem {
  constructor(lifecycle) {
    this.lifecycle = lifecycle;
    this.registered = {};
  }
  apply(plugin) {
    if (!this.registered[plugin.name]) {
      this.registered[plugin.name] = plugin;
      for (const key of Object.keys(this.lifecycle)) if (plugin[key]) this.lifecycle[key].on(plugin[key]);
    }
  }
  remove(name) {
    const plugin = this.registered[name];
    if (!plugin) return;
    for (const key of Object.keys(this.lifecycle)) if (plugin[key]) this.lifecycle[key].remove(plugin[key]);
    delete this.registered[name];
  }
}

// ---- remotes -------------------------------------------------------------------------------

// What a remote is called and where it lives, with the defaults filled in.
function remoteInfo(remote) {
  const info = { ...remote };
  info.type ||= 'global';
  info.entryGlobalName ||= info.name;
  info.shareScope ||= DEFAULT_SCOPE;
  if (!info.entry && info.version) fail('RUNTIME-011', `The remote "${info.name}" has no entry; "version" needs a registry, which is not supported.`);
  return info;
}

// `app/Button` is the remote `app` and its exposed `./Button`. The longest name wins, and an alias
// can stand for the name.
function matchRemote(remotes, id) {
  let best;
  for (const remote of remotes) {
    for (const key of [remote.name, remote.alias]) {
      if (!key || !(id === key || id.startsWith(key + '/'))) continue;
      if (!best || key.length > best.key.length) best = { key, remote };
    }
  }
  if (!best) return undefined;
  const rest = id.slice(best.key.length);
  return { pkgNameOrAlias: best.key, remote: best.remote, expose: rest ? '.' + rest : '.', id };
}

const fileUrl = (base, file) => {
  try {
    return new URL(file, base).href;
  } catch {
    return base.replace(/[^/]*$/, '') + file;
  }
};

// Reads a remote's manifest and works out where its entry file is and how to load it.
async function readManifest(host, url) {
  const loading = (registry.__MANIFEST_LOADING__[url] ||= (async () => {
    const response = await host.loaderHook.lifecycle.fetch.emit(url, {});
    const res = response instanceof Response || (response && typeof response.json === 'function') ? response : await fetch(url, {});
    if (!res.ok) fail('RUNTIME-003', `Failed to get the manifest ${url} (${res.status}).`);
    const manifest = await res.json();
    if (!manifest || !manifest.metaData || !manifest.metaData.remoteEntry) fail('RUNTIME-013', `${url} is not a valid manifest.`);
    return manifest;
  })());
  try {
    return await loading;
  } catch (error) {
    delete registry.__MANIFEST_LOADING__[url];
    throw error;
  }
}

function manifestInfo(manifest, url) {
  const { metaData } = manifest;
  const base = !metaData.publicPath || metaData.publicPath === 'auto' ? url : metaData.publicPath.endsWith('/') ? metaData.publicPath : metaData.publicPath + '/';
  return {
    entry: fileUrl(base, (metaData.remoteEntry.path ? metaData.remoteEntry.path.replace(/\/?$/, '/') : '') + metaData.remoteEntry.name),
    type: metaData.remoteEntry.type || 'global',
    globalName: metaData.globalName || metaData.name,
    publicPath: base,
  };
}

// ---- loading entry files -------------------------------------------------------------------

const scripts = new Map();

function loadScript(host, url, attrs = {}) {
  if (scripts.has(url)) return scripts.get(url);
  const promise = new Promise((resolve, reject) => {
    const made = host.loaderHook.lifecycle.createScript.emit({ url, attrs });
    const script = made && made.script ? made.script : document.createElement('script');
    if (!(made && made.script)) {
      script.src = url;
      script.async = true;
      script.crossOrigin = (made && made.attrs && made.attrs.crossOrigin) || attrs.crossOrigin || 'anonymous';
    }
    script.onload = () => resolve();
    script.onerror = () => reject(new Error(`Failed to load ${url}`));
    document.head.appendChild(script);
  });
  promise.catch(() => scripts.delete(url));
  scripts.set(url, promise);
  return promise;
}

// Reads the script and runs it in the global scope, where a browser would.
async function runScript(host, url) {
  const response = await host.loaderHook.lifecycle.fetch.emit(url, {});
  const res = response && typeof response.text === 'function' ? response : await fetch(url, {});
  if (!res.ok) fail('RUNTIME-008', `Failed to load ${url} (${res.status}).`);
  (0, eval)(`${await res.text()}\n//# sourceURL=${url}`);
}

async function importEntry(url) {
  return import(/* @vite-ignore */ url);
}

async function loadEntry(host, info) {
  const emitted = await host.remoteHandler.hooks.lifecycle.loadEntry.emit({ loaderHook: host.loaderHook, remoteInfo: info, remoteEntryExports: undefined });
  if (emitted && typeof emitted === 'object' && (emitted.init || emitted.get)) return emitted;
  let { entry, type, entryGlobalName } = info;
  // An address written as code gives the container, or a promise of it.
  if (typeof info.load === 'function') return info.load();
  if (/\.json(?:$|[?#])/.test(entry)) {
    const manifest = await readManifest(host, entry);
    const found = manifestInfo(manifest, entry);
    registry.moduleInfo[info.name] = { ...found, modules: manifest.exposes, shared: manifest.shared };
    info.manifest = manifest;
    info.publicPath = found.publicPath;
    entry = info.entryUrl = found.entry;
    type = info.type = info.declaredType || found.type;
    entryGlobalName = info.entryGlobalName = info.declaredGlobalName || found.globalName;
  }
  try {
    if (type === 'module' || type === 'esm') {
      const exports = await importEntry(entry);
      return exports.get || exports.init ? exports : exports.default;
    }
    if (type === 'jsonp') {
      // The script calls a global function named like the remote with its container.
      return await new Promise((resolve, reject) => {
        root[entryGlobalName] = resolve;
        (isBrowser() ? loadScript(host, entry) : runScript(host, entry)).catch(reject);
      });
    }
    if (type === 'system' || type === 'systemjs') {
      if (!root.System || typeof root.System.import !== 'function') fail('RUNTIME-001', `${entry} is a SystemJS module and SystemJS is not on the page.`);
      return await root.System.import(entry);
    }
    if (type === 'commonjs' || type === 'cjs') {
      const { createRequire } = await import(/* @vite-ignore */ 'node:module');
      return createRequire(import.meta.url)(entry);
    }
    if (!root[entryGlobalName]) {
      if (isBrowser()) await loadScript(host, entry);
      else await runScript(host, entry);
    }
    const container = root[entryGlobalName];
    if (!container) fail('RUNTIME-001', `${entry} did not define the global "${entryGlobalName}".`);
    return container;
  } catch (error) {
    const recovered = await host.loaderHook.lifecycle.loadEntryError.emit({ getRemoteEntry: (i) => loadEntry(host, remoteInfo(i)), origin: host, remoteInfo: info, remoteEntryExports: undefined, globalLoading: scripts, uniqueKey: `${entryGlobalName}:${entry}` });
    if (recovered) return recovered;
    throw error;
  }
}

// The style sheets an exposed module needs are named in the manifest.
function addStyles(info, expose) {
  if (!isBrowser() || !info.manifest || !info.publicPath) return;
  const found = (info.manifest.exposes || []).find((e) => './' + e.name === expose || e.path === expose);
  const files = found && found.assets && found.assets.css ? [...(found.assets.css.sync || []), ...(found.assets.css.async || [])] : [];
  for (const file of files) {
    const href = fileUrl(info.publicPath, file);
    if (document.querySelector(`link[rel="stylesheet"][href="${href}"]`)) continue;
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = href;
    document.head.appendChild(link);
  }
}

class RemoteModule {
  constructor(host, info) {
    this.host = host;
    this.remoteInfo = info;
    this.container = undefined;
    this.inited = false;
    this.initPromise = undefined;
  }

  async getEntry() {
    if (!this.container) {
      this.container = await loadEntry(this.host, this.remoteInfo);
      if (!this.container) fail('RUNTIME-001', `The entry of "${this.remoteInfo.name}" exported nothing.`);
    }
    return this.container;
  }

  async init(initScope) {
    const container = await this.getEntry();
    if (this.inited) return container;
    if (this.initPromise) {
      await this.initPromise;
      return container;
    }
    this.initPromise = (async () => {
      const { host } = this;
      await host.loaderHook.lifecycle.beforeInitRemote.emit({ remoteInfo: this.remoteInfo, origin: host });
      const keys = toArray(this.remoteInfo.shareScope);
      if (!keys.length) keys.push(DEFAULT_SCOPE);
      for (const key of keys) host.shareScopeMap[key] ||= {};
      const options = { version: this.remoteInfo.version || '', shareScopeKeys: Array.isArray(this.remoteInfo.shareScope) ? keys : this.remoteInfo.shareScope || DEFAULT_SCOPE };
      Object.defineProperty(options, 'shareScopeMap', { value: host.shareScopeMap, enumerable: false });
      const args = await host.hooks.lifecycle.beforeInitContainer.emit({ shareScope: host.shareScopeMap[keys[0]], remoteEntryInitOptions: options, initScope: initScope || [], remoteInfo: this.remoteInfo, origin: host });
      if (typeof container.init !== 'function') fail('RUNTIME-002', `The entry of "${this.remoteInfo.name}" has no "init".`);
      try {
        await container.init(args.shareScope, args.initScope, args.remoteEntryInitOptions);
      } catch (error) {
        fail('RUNTIME-015', `Initializing the remote "${this.remoteInfo.name}" failed: ${error && error.message}`);
      }
      await host.hooks.lifecycle.initContainer.emit({ ...args, remoteEntryExports: container, origin: host });
      this.inited = true;
      await host.loaderHook.lifecycle.afterInitRemote.emit({ remoteInfo: this.remoteInfo, remoteEntryExports: container, origin: host });
    })();
    try {
      await this.initPromise;
    } finally {
      this.initPromise = undefined;
    }
    return container;
  }

  async get(id, expose, options) {
    const container = await this.init();
    const { host } = this;
    await host.loaderHook.lifecycle.beforeGetExpose.emit({ id, expose, moduleInfo: this.remoteInfo, remoteEntryExports: container, origin: host });
    let factory = await host.loaderHook.lifecycle.getModuleFactory.emit({ remoteEntryExports: container, expose, moduleInfo: this.remoteInfo });
    if (typeof factory !== 'function') factory = await container.get(expose);
    if (typeof factory !== 'function') fail('RUNTIME-014', `The remote "${this.remoteInfo.name}" does not expose "${expose}".`);
    await host.loaderHook.lifecycle.afterGetExpose.emit({ id, expose, moduleInfo: this.remoteInfo, remoteEntryExports: container, moduleFactory: factory, origin: host });
    addStyles(this.remoteInfo, expose);
    if (options && options.loadFactory === false) return factory;
    await host.loaderHook.lifecycle.beforeExecuteFactory.emit({ id, expose, moduleInfo: this.remoteInfo, origin: host });
    const exposed = await factory();
    await host.loaderHook.lifecycle.afterExecuteFactory.emit({ id, expose, moduleInfo: this.remoteInfo, exposeContent: exposed, origin: host });
    return exposed;
  }
}

class RemoteHandler {
  constructor(host) {
    this.host = host;
    this.hooks = new PluginSystem({
      beforeRegisterRemote: new SyncWaterfallHook('beforeRegisterRemote'),
      registerRemote: new SyncWaterfallHook('registerRemote'),
      beforeRequest: new AsyncWaterfallHook('beforeRequest'),
      afterMatchRemote: new AsyncHook('afterMatchRemote'),
      onLoad: new AsyncHook('onLoad'),
      afterLoadRemote: new AsyncHook('afterLoadRemote'),
      handlePreloadModule: new SyncHook('handlePreloadModule'),
      errorLoadRemote: new AsyncHook('errorLoadRemote'),
      beforePreloadRemote: new AsyncHook('beforePreloadRemote'),
      generatePreloadAssets: new AsyncHook('generatePreloadAssets'),
      afterPreloadRemote: new AsyncHook('afterPreloadRemote'),
      loadEntry: new AsyncHook('loadEntry'),
    });
  }

  register(remote, list, { force } = {}) {
    const { host } = this;
    remote = this.hooks.lifecycle.beforeRegisterRemote.emit({ remote, origin: host }).remote;
    const same = list.findIndex((r) => r.name === remote.name);
    if (same >= 0 && !force) {
      if (list[same].entry !== remote.entry) warn(`The remote "${remote.name}" is already registered with another entry; pass { force: true } to replace it.`);
      return;
    }
    if (same >= 0) {
      host.moduleCache.delete(remote.name);
      list.splice(same, 1);
    }
    remote = { ...remote };
    if (remote.type) remote.declaredType = remote.type;
    if (remote.entryGlobalName) remote.declaredGlobalName = remote.entryGlobalName;
    list.push(this.hooks.lifecycle.registerRemote.emit({ remote, origin: host }).remote);
  }

  async resolve(id) {
    const { host } = this;
    let args;
    try {
      args = await this.hooks.lifecycle.beforeRequest.emit({ id, options: host.options, origin: host });
    } catch (error) {
      args = await this.hooks.lifecycle.errorLoadRemote.emit({ id, options: host.options, origin: host, from: 'runtime', error, lifecycle: 'beforeRequest' });
      if (!args) throw error;
    }
    const matched = matchRemote(host.options.remotes, args.id);
    if (!matched) fail('RUNTIME-004', `No remote matches "${args.id}" in "${host.options.name}".`);
    const info = remoteInfo(matched.remote);
    await this.hooks.lifecycle.afterMatchRemote.emit({ id: args.id, ...matched, options: host.options, remoteInfo: info, origin: host });
    const after = await host.sharedHandler.hooks.lifecycle.afterResolve.emit({ id: args.id, ...matched, options: host.options, origin: host, remoteInfo: info });
    let module = host.moduleCache.get(after.remote.name);
    if (!module) {
      module = new RemoteModule(host, remoteInfo(after.remote));
      host.moduleCache.set(after.remote.name, module);
    }
    return { module, expose: after.expose, id: after.id || args.id, remote: after.remote, pkgNameOrAlias: matched.pkgNameOrAlias };
  }

  async loadRemote(id, options) {
    const { host } = this;
    let failed;
    try {
      const { module, expose, id: resolved, remote, pkgNameOrAlias } = await this.resolve(id);
      const loadFactory = !options || options.loadFactory !== false;
      const exposed = await module.get(resolved, expose, options);
      const wrapper = await this.hooks.lifecycle.onLoad.emit({ id: resolved, pkgNameOrAlias, expose, exposeModule: loadFactory ? exposed : undefined, exposeModuleFactory: loadFactory ? undefined : exposed, remote, options: { host, remoteInfo: module.remoteInfo }, moduleInstance: module, origin: host });
      await this.hooks.lifecycle.afterLoadRemote.emit({ id: resolved, expose, remote, options, origin: host });
      return typeof wrapper === 'function' ? wrapper : exposed;
    } catch (error) {
      failed = error;
      const recovered = await this.hooks.lifecycle.errorLoadRemote.emit({ id, error, from: (options && options.from) || 'runtime', lifecycle: 'onLoad', remote: undefined, origin: host });
      if (!recovered) throw error;
      await this.hooks.lifecycle.afterLoadRemote.emit({ id, options, error, origin: host, recovered: true });
      return recovered;
    } finally {
      void failed;
    }
  }

  async preloadRemote(list) {
    const { host } = this;
    await this.hooks.lifecycle.beforePreloadRemote.emit({ preloadOps: list, options: host.options, origin: host });
    const results = await Promise.allSettled(
      toArray(list).map(async (op) => {
        const matched = matchRemote(host.options.remotes, op.nameOrAlias);
        if (!matched) fail('RUNTIME-004', `No remote matches "${op.nameOrAlias}".`);
        const module = host.moduleCache.get(matched.remote.name) || new RemoteModule(host, remoteInfo(matched.remote));
        host.moduleCache.set(matched.remote.name, module);
        await module.init();
        if (isBrowser() && module.remoteInfo.manifest) for (const e of module.remoteInfo.manifest.exposes || []) if (!op.exposes || op.exposes.includes('./' + e.name)) addStyles(module.remoteInfo, './' + e.name);
      }),
    );
    const error = results.find((r) => r.status === 'rejected');
    await this.hooks.lifecycle.afterPreloadRemote.emit({ preloadOps: list, options: host.options, origin: host, error: error && error.reason });
    if (error) throw error.reason;
  }
}

// ---- shared packages -----------------------------------------------------------------------

// `include` and `exclude` say which copies of a package may be shared: by the version of the copy bundled here, and by
// the request. A copy that is left out is used by this application alone.
function leftOut({ version = '0', include, exclude }, name) {
  const matches = (rule) => {
    if (!rule) return true;
    if (rule.version !== undefined && !satisfy(rule.fallbackVersion || version, rule.version)) return false;
    if (rule.request !== undefined && !(rule.request.__regexp === undefined ? new RegExp(rule.request) : new RegExp(rule.request.__regexp, rule.request.flags)).test(name)) return false;
    return true;
  };
  if (include && !matches(include)) return true;
  return Boolean(exclude && Object.keys(exclude).length && matches(exclude));
}

function formatShare(args, from, name, strategy) {
  let get;
  if ('get' in args) get = args.get;
  else if ('lib' in args) get = () => Promise.resolve(args.lib);
  else get = () => Promise.resolve(() => fail('RUNTIME-012', `Cannot get the shared "${name}" from "${from}": it has neither "get" nor "lib".`));
  const { version = '0' } = args;
  return {
    deps: [],
    useIn: [],
    from,
    loading: null,
    ...args,
    shareConfig: { requiredVersion: `^${version}`, singleton: false, eager: false, strictVersion: false, ...args.shareConfig },
    get,
    loaded: args.loaded || 'lib' in args ? true : undefined,
    version,
    scope: Array.isArray(args.scope) ? args.scope : [args.scope || DEFAULT_SCOPE],
    strategy: args.strategy || strategy || 'version-first',
    treeShaking: args.treeShaking ? { ...args.treeShaking, useIn: [] } : undefined,
    local: leftOut({ version: args.version, include: (args.shareConfig || {}).include ?? args.include, exclude: (args.shareConfig || {}).exclude ?? args.exclude }, name),
  };
}

const isLoaded = (s) => Boolean(s.loaded) || typeof s.lib === 'function';
const isLoadingOrLoaded = (s) => isLoaded(s) || Boolean(s.loading);

// The version of the package to settle on when only one copy may run: the newest that has not
// loaded yet (`version-first`), or one already loading or loaded, then the newest (`loaded-first`).
function pickVersion(versions, strategy) {
  const names = Object.keys(versions);
  const lt = (a, b) => lessThan(a, b);
  const before = (prev, cur) => {
    if (strategy === 'loaded-first') {
      if (isLoadingOrLoaded(versions[cur])) return isLoadingOrLoaded(versions[prev]) ? lt(prev, cur) : true;
      if (isLoadingOrLoaded(versions[prev])) return false;
      return lt(prev, cur);
    }
    return !isLoaded(versions[prev]) && lt(prev, cur);
  };
  return names.reduce((prev, cur) => (!prev ? cur : before(prev, cur) ? cur : prev === '0' ? cur : prev), '');
}

function findRegisteredShare(scopeMap, name, info, resolveShare) {
  const { shareConfig, scope = DEFAULT_SCOPE, strategy } = info;
  for (const sc of toArray(scope)) {
    if (!shareConfig || !scopeMap[sc] || !scopeMap[sc][name]) continue;
    const { requiredVersion } = shareConfig;
    const versions = scopeMap[sc][name];
    const chosen = pickVersion(versions, strategy);
    const resolver = () => {
      const shared = versions[chosen];
      if (shareConfig.singleton) {
        if (typeof requiredVersion === 'string' && !satisfy(chosen, requiredVersion)) {
          const message = `Version ${chosen} from ${shared.from} of the shared singleton ${name} does not satisfy the requirement of ${info.from}, which needs ${requiredVersion}.`;
          if (shareConfig.strictVersion) fail('RUNTIME-000', message);
          warn(message);
        }
        return { shared };
      }
      if (requiredVersion === false || requiredVersion === '*' || satisfy(chosen, requiredVersion)) return { shared };
      for (const [version, value] of Object.entries(versions)) if (satisfy(version, requiredVersion)) return { shared: value };
      return undefined;
    };
    const params = { shareScopeMap: scopeMap, scope: sc, pkgName: name, version: chosen, shareInfo: info, resolver };
    return (resolveShare.emit(params) || params).resolver();
  }
  return undefined;
}

class SharedHandler {
  constructor(host) {
    this.host = host;
    this.initTokens = {};
    this.hooks = new PluginSystem({
      beforeRegisterShare: new SyncWaterfallHook('beforeRegisterShare'),
      afterRegisterShare: new SyncHook('afterRegisterShare'),
      afterResolve: new AsyncWaterfallHook('afterResolve'),
      beforeLoadShare: new AsyncWaterfallHook('beforeLoadShare'),
      loadShare: new AsyncHook('loadShare'),
      afterLoadShare: new SyncHook('afterLoadShare'),
      errorLoadShare: new SyncHook('errorLoadShare'),
      resolveShare: new SyncWaterfallHook('resolveShare'),
      initContainerShareScopeMap: new SyncWaterfallHook('initContainerShareScopeMap'),
    });
  }

  afterRegister(name, input) {
    this.hooks.lifecycle.afterRegisterShare.emit({ pkgName: name, ...input, shareScopeMap: this.host.shareScopeMap, origin: this.host });
  }

  // Adds the host's own shared packages to the options, and to the scopes when they are new.
  registerShared(current, user) {
    const added = {};
    const all = { ...current.shared };
    for (const [name, list] of Object.entries(user.shared || {})) {
      added[name] = toArray(list).map((s) => formatShare(s, user.name, name, user.shareStrategy));
      if (!all[name]) all[name] = added[name];
      else for (const s of added[name]) if (!all[name].some((x) => x.version === s.version)) all[name].push(s);
    }
    for (const [name, list] of Object.entries(added)) {
      for (const shared of list) {
        for (const sc of shared.scope) {
          this.hooks.lifecycle.beforeRegisterShare.emit({ origin: this.host, pkgName: name, shared });
          const registered = this.host.shareScopeMap[sc] && this.host.shareScopeMap[sc][name];
          const before = registered && registered[shared.version];
          if (!registered && !shared.consumeOnly && !shared.local) this.set(name, sc, shared, { loaded: shared.loaded || Boolean(shared.lib), from: user.name });
          this.afterRegister(name, { scope: sc, shared, previousShared: before, registeredShared: this.host.shareScopeMap[sc] && this.host.shareScopeMap[sc][name] && this.host.shareScopeMap[sc][name][shared.version], trigger: 'runtime' });
        }
      }
    }
    return all;
  }

  set(name, scopeName, shared, patch = {}) {
    const scopes = this.host.shareScopeMap;
    const scope = (scopes[scopeName] ||= {});
    const versions = (scope[name] ||= {});
    const target = (versions[shared.version] ||= shared);
    Object.assign(target, patch.loaded !== undefined ? { loaded: patch.loaded } : {}, patch.from ? { from: patch.from } : {}, 'lib' in patch ? { lib: patch.lib } : {}, 'loading' in patch ? { loading: patch.loading } : {});
    if (patch.lib && !target.get) target.get = () => Promise.resolve(patch.lib);
  }

  // Puts what the host shares into the scope, once per scope; with `version-first`, starts the remotes.
  initializeSharing(scopeName = DEFAULT_SCOPE, extra = {}) {
    const { host } = this;
    const { from, strategy } = extra;
    const promises = [];
    let initScope = extra.initScope;
    if (from !== 'build') {
      initScope ||= [];
      const token = (this.initTokens[scopeName] ||= { from: host.options.name });
      if (initScope.indexOf(token) >= 0) return promises;
      initScope.push(token);
    }
    const scopes = host.shareScopeMap;
    const scope = (scopes[scopeName] ||= {});
    const name = host.options.name;
    const register = (pkg, shared) => {
      const { version, eager } = shared;
      const versions = (scope[pkg] ||= {});
      const existing = versions[version];
      const eagerOf = (s) => Boolean(s && ((s.eager) || (s.shareConfig && s.shareConfig.eager)));
      const replace = !existing || (existing.strategy !== 'loaded-first' && !existing.loaded && (Boolean(!eager) !== !eagerOf(existing) ? eager : name > existing.from));
      if (replace) versions[version] = shared;
      this.afterRegister(pkg, { scope: scopeName, shared, previousShared: existing, registeredShared: versions[version], trigger: from || 'runtime' });
    };
    for (const [pkg, list] of Object.entries(host.options.shared)) for (const shared of list) if (shared.scope.includes(scopeName) && !shared.consumeOnly && !shared.local) register(pkg, shared);
    if (host.options.shareStrategy === 'version-first' || strategy === 'version-first') {
      for (const remote of host.options.remotes) {
        // A container that is a module of this application shares nothing of its own, so it is not started to find out what.
        if (!remote.internal && toArray(remote.shareScope || DEFAULT_SCOPE).includes(scopeName)) promises.push(this.initRemote(remote.name, initScope));
      }
    }
    return promises;
  }

  // Starts a remote so that what it shares is in the scope before anything is chosen.
  async initRemote(name, initScope) {
    const { host } = this;
    let module = host.moduleCache.get(name);
    if (!module) {
      const remote = host.options.remotes.find((r) => r.name === name);
      module = new RemoteModule(host, remoteInfo(remote));
      host.moduleCache.set(name, module);
    }
    let container;
    try {
      container = await module.getEntry();
    } catch (error) {
      container = await host.remoteHandler.hooks.lifecycle.errorLoadRemote.emit({ id: name, error, from: 'runtime', lifecycle: 'beforeLoadShare', remote: module.remoteInfo, origin: host });
      if (!container) return;
      module.container = container;
    }
    if (container && container.init && !module.initPromise) await module.init(initScope);
  }

  async loadShare(name, extra = {}) {
    const { host } = this;
    let info = pickShareOptions(name, extra, host.options.shared);
    try {
      if (info && info.scope) await Promise.all(info.scope.map((sc) => Promise.all(this.initializeSharing(sc, { strategy: info.strategy }))));
      const before = await this.hooks.lifecycle.beforeLoadShare.emit({ pkgName: name, shareInfo: info, shared: host.options.shared, origin: host, loadContext: extra.context });
      info = before.shareInfo;
      if (!info) fail('RUNTIME-009', `"${name}" is not shared by "${host.options.name}".`);
      if (info.local) {
        // Left out of sharing by include / exclude: this application's own copy, found without asking the scope.
        info.loading ||= Promise.resolve(info.get()).then((factory) => (info.lib = factory));
        return info.loading;
      }
      const found = findRegisteredShare(host.shareScopeMap, name, info, this.hooks.lifecycle.resolveShare);
      // A copy that has only some of the exports is used when it has all that this application reads.
      const wanted = info.treeShaking && info.treeShaking.usedExports;
      const variantOf = (entry) => (wanted && entry.treeShaking && typeof entry.treeShaking.get === 'function' && wanted.every((n) => (entry.treeShaking.usedExports || []).includes(n)) ? entry.treeShaking : entry);
      const target = found && found.shared && variantOf(found.shared);
      const use = (shared) => {
        const users = (shared.useIn ||= []);
        if (!users.includes(host.options.name)) users.push(host.options.name);
      };
      if (target) {
        if (target.lib) {
          use(target);
          return target.lib;
        }
        if (target.loading && !target.loaded) {
          const factory = await target.loading;
          target.loaded = true;
          target.lib ||= factory;
          use(target);
          return factory;
        }
        const loading = (async () => {
          const factory = await target.get();
          use(target);
          target.loaded = true;
          target.lib = factory;
          return factory;
        })();
        target.loading = loading;
        const factory = await loading;
        this.hooks.lifecycle.afterLoadShare.emit({ pkgName: name, shareInfo: info, selectedShared: target, origin: host });
        return factory;
      }
      if (extra.customShareInfo) return false;
      if (info.consumeOnly) fail('RUNTIME-012', `The shared "${name}" is not bundled in "${host.options.name}" and no other application provides it.`);
      // Nothing registered one: use the host's own copy.
      const own = variantOf(info);
      const loading = (async () => {
        const factory = await own.get();
        own.lib = factory;
        own.loaded = true;
        use(own);
        return factory;
      })();
      own.loading = loading;
      for (const sc of info.scope) this.set(name, sc, info, { loaded: false, from: host.options.name });
      const factory = await loading;
      this.hooks.lifecycle.afterLoadShare.emit({ pkgName: name, shareInfo: info, selectedShared: info, origin: host });
      return factory;
    } catch (error) {
      this.hooks.lifecycle.errorLoadShare.emit({ pkgName: name, shareInfo: info, error, origin: host });
      throw error;
    }
  }

  // Only for a package whose copy is already there; a copy that is still to be fetched cannot be read synchronously.
  loadShareSync(name, extra = {}) {
    const { host } = this;
    const info = pickShareOptions(name, extra, host.options.shared);
    if (info && info.scope) for (const sc of info.scope) this.initializeSharing(sc, { strategy: info.strategy, from: extra.from });
    const found = info && findRegisteredShare(host.shareScopeMap, name, info, this.hooks.lifecycle.resolveShare);
    const target = found && found.shared;
    if (target && typeof target.lib === 'function') {
      target.loaded = true;
      return target.lib;
    }
    if (target && typeof target.get === 'function') {
      const value = target.get();
      if (!(value instanceof Promise)) {
        target.lib = value;
        target.loaded = true;
        return value;
      }
    }
    if (info && info.lib) return info.lib;
    fail('RUNTIME-006', `The shared "${name}" cannot be read synchronously from "${host.options.name}".`);
  }
}

// The host's own entry for a package: the one that matches its version when it is told which.
function pickShareOptions(name, extra, shared) {
  const options = shared[name];
  const list = toArray(extra.customShareInfo ? [extra.customShareInfo] : options);
  if (!list.length) return undefined;
  const merged = list.map((s) => (extra.customShareInfo ? { ...s, ...formatShare(s, s.from || '', name) } : s));
  const byVersion = {};
  for (const s of merged) byVersion[s.version] = s;
  return byVersion[pickVersion(byVersion, 'version-first')];
}

// ---- the host ------------------------------------------------------------------------------

function dedupeRemotes(handler, remotes) {
  const list = [];
  for (const remote of remotes || []) handler.register(remote, list);
  return list;
}

export class FederationHost {
  constructor(user) {
    this.options = { id: user.id || user.name, name: user.name, version: user.version, remotes: [], shared: {}, plugins: [], inBrowser: isBrowser(), shareStrategy: user.shareStrategy || 'version-first' };
    this.hooks = new PluginSystem({
      beforeInit: new SyncWaterfallHook('beforeInit'),
      init: new SyncHook('init'),
      beforeInitContainer: new AsyncWaterfallHook('beforeInitContainer'),
      initContainer: new AsyncWaterfallHook('initContainer'),
    });
    this.loaderHook = new PluginSystem({
      getModuleInfo: new SyncHook('getModuleInfo'),
      createScript: new SyncHook('createScript'),
      createLink: new SyncHook('createLink'),
      fetch: new AsyncHook('fetch'),
      loadEntryError: new AsyncHook('loadEntryError'),
      getModuleFactory: new AsyncHook('getModuleFactory'),
      beforeInitRemote: new AsyncHook('beforeInitRemote'),
      afterInitRemote: new AsyncHook('afterInitRemote'),
      beforeGetExpose: new AsyncHook('beforeGetExpose'),
      afterGetExpose: new AsyncHook('afterGetExpose'),
      beforeExecuteFactory: new AsyncHook('beforeExecuteFactory'),
      afterExecuteFactory: new AsyncHook('afterExecuteFactory'),
    });
    this.remoteHandler = new RemoteHandler(this);
    this.sharedHandler = new SharedHandler(this);
    this.moduleCache = new Map();
    this.shareScopeMap = {};
    registry.__SHARE__[this.options.id] = this.shareScopeMap;
    registry.__INSTANCES__.push(this);
    this.registerPlugins([...registry.__GLOBAL_PLUGIN__, ...toArray(user.plugins)]);
    this.initOptions(user);
  }

  registerPlugins(plugins) {
    for (const plugin of plugins) {
      if (!isObject(plugin) || !plugin.name) fail('RUNTIME-000', 'A plugin needs a name.');
      for (const system of [this.hooks, this.loaderHook, this.remoteHandler.hooks, this.sharedHandler.hooks]) system.apply(plugin);
      if (!this.options.plugins.includes(plugin)) this.options.plugins.push(plugin);
    }
  }

  initOptions(user) {
    const { options } = this.hooks.lifecycle.beforeInit.emit({ origin: this, userOptions: user, options: this.options, shareInfo: this.options.shared });
    this.options = options;
    options.remotes = dedupeRemotes(this.remoteHandler, [...options.remotes, ...(user.remotes || [])]);
    options.shared = this.sharedHandler.registerShared(options, { name: user.name, shared: user.shared, shareStrategy: options.shareStrategy });
    this.hooks.lifecycle.init.emit({ origin: this, options });
    return this;
  }

  // Adds more of what the host knows, as `init` does for a host that already exists.
  extend(user) {
    this.registerPlugins(toArray(user.plugins));
    this.options.remotes = dedupeRemotes(this.remoteHandler, [...this.options.remotes, ...(user.remotes || [])]);
    this.options.shared = this.sharedHandler.registerShared(this.options, { name: this.options.name, shared: user.shared, shareStrategy: this.options.shareStrategy });
    return this;
  }

  loadRemote(id, options) {
    return this.remoteHandler.loadRemote(id, options);
  }
  preloadRemote(list) {
    return this.remoteHandler.preloadRemote(list);
  }
  registerRemotes(remotes, options) {
    for (const remote of remotes) this.remoteHandler.register(remote, this.options.remotes, options);
  }
  loadShare(name, extra) {
    return this.sharedHandler.loadShare(name, extra);
  }
  loadShareSync(name, extra) {
    return this.sharedHandler.loadShareSync(name, extra);
  }
  registerShared(shared) {
    this.options.shared = this.sharedHandler.registerShared(this.options, { name: this.options.name, shared, shareStrategy: this.options.shareStrategy });
  }
  initializeSharing(scope, extra) {
    return this.sharedHandler.initializeSharing(scope, extra);
  }
  initShareScopeMap(scopeName, shareScope, extra = {}) {
    this.shareScopeMap[scopeName] = shareScope;
    registry.__SHARE__[this.options.id] = this.shareScopeMap;
    this.sharedHandler.hooks.lifecycle.initContainerShareScopeMap.emit({ shareScope, options: this.options, origin: this, scopeName, hostShareScopeMap: extra.hostShareScopeMap });
  }
}

let current;

export function getInstance() {
  return current || registry.__INSTANCES__[registry.__INSTANCES__.length - 1];
}

export function createInstance(options) {
  const existing = registry.__INSTANCES__.find((i) => i instanceof FederationHost && i.options.name === options.name && options.name);
  if (existing) return existing.extend(options);
  return new FederationHost(options);
}

// The host of the application being built: one per name, however often this is called.
export function init(options) {
  const existing = registry.__INSTANCES__.find((i) => i instanceof FederationHost && i.options.name === options.name);
  current = existing ? existing.extend(options) : new FederationHost(options);
  return current;
}

const need = () => getInstance() || fail('RUNTIME-009', 'Call init first.');
export const loadRemote = (id, options) => need().loadRemote(id, options);
export const loadShare = (name, options) => need().loadShare(name, options);
export const loadShareSync = (name, options) => need().loadShareSync(name, options);
export const preloadRemote = (list) => need().preloadRemote(list);
export const registerRemotes = (remotes, options) => need().registerRemotes(remotes, options);
export const registerPlugins = (plugins) => need().registerPlugins(plugins);
export const registerShared = (shared) => need().registerShared(shared);

export function registerGlobalPlugins(plugins) {
  for (const plugin of toArray(plugins)) if (!registry.__GLOBAL_PLUGIN__.some((p) => p.name === plugin.name)) registry.__GLOBAL_PLUGIN__.push(plugin);
}

// ---- what a build links to ------------------------------------------------------------------

const isNamespace = (m) => m !== null && typeof m === 'object' && m[Symbol.toStringTag] === 'Module';

// The namespace of an ES module seen as `require` and `import x from` see a module that says it
// is one (`__esModule`): its default is its `default` export.
function flagged(ns) {
  const view = Object.create(null, { [Symbol.toStringTag]: { value: 'Module' }, __esModule: { value: true } });
  for (const key of Object.keys(ns)) Object.defineProperty(view, key, { get: () => ns[key], enumerable: true });
  return view;
}

// A module another application exposes, as this application's code reads it.
export function moduleValue(m) {
  return isNamespace(m) && !m.__esModule ? flagged(m) : m;
}

// A package bundled here as a fallback. A CommonJS package arrives as a namespace that mirrors its
// exports object, and is read as that object; anything else is an ES module.
export function fallbackValue(m) {
  if (!isNamespace(m) || m.__esModule) return m;
  const exports = m.default;
  if ((typeof exports === 'object' || typeof exports === 'function') && exports !== null && Object.keys(m).every((key) => key === 'default' || m[key] === exports[key])) return exports;
  return flagged(m);
}

// A loaded module as `import()` gives it: the namespace of an ES module, or the exports of a
// CommonJS one with that as its default.
export function moduleNamespace(m) {
  if (isNamespace(m)) return m;
  if (m !== null && typeof m === 'object' && m.__esModule) return Object.assign(Object.create(null, { [Symbol.toStringTag]: { value: 'Module' } }), m);
  return m !== null && (typeof m === 'object' || typeof m === 'function') ? Object.assign(Object.create(null, { [Symbol.toStringTag]: { value: 'Module' } }), m, { default: m }) : { default: m };
}

// What this application offers others: `get` and `init`, which a host calls on the container it loaded.
export function containerFor(host, config) {
  const exposes = config.exposes || {};
  return {
    async get(expose) {
      const load = exposes[expose];
      if (!load) throw new Error(`Module "${expose}" does not exist in container.`);
      if (config.ready) await config.ready();
      const m = await load();
      // Other bundlers take a module for an ES module when it says so.
      return () => (isNamespace(m) && !m.__esModule ? flagged(m) : m);
    },
    async init(shareScope, initScope, options = {}) {
      const keys = toArray(options.shareScopeKeys && options.shareScopeKeys.length ? options.shareScopeKeys : config.shareScope && config.shareScope.length ? config.shareScope : DEFAULT_SCOPE);
      const hostMap = options.shareScopeMap;
      keys.forEach((key, i) => host.initShareScopeMap(key, (hostMap && hostMap[key]) || (i === 0 ? shareScope : {}), { hostShareScopeMap: hostMap }));
      const strategy = host.options.shareStrategy;
      for (const key of keys) await Promise.all(host.initializeSharing(key, { strategy, from: 'build', initScope }));
    },
  };
}

export function createContainer(config) {
  return containerFor(init(config), config);
}
