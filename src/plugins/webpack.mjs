// webpack plugins for the plugin host: a compiler and a compilation that webpack plugins can `apply` to.
//
// This file is appended to host.mjs and shares its scope (`root`, `cli`, `log`, `emitted`, ...). A
// webpack plugin is an object with `apply(compiler)` that taps the compiler's hooks. Here the hooks are
// real (a small Tapable), and the build of rathkit-pack drives them in webpack's order where that makes
// sense for a bundler that works differently:
//
//   buildStart      environment ... beforeCompile, compile, thisCompilation, compilation, make
//   moduleGraph     buildModule / succeedModule for every module, finishModules
//   generateBundle  seal, optimize*, processAssets (by stage), afterProcessAssets, afterSeal, afterCompile, emit
//   writeBundle     assetEmitted, afterEmit, done
//
// Assets are what the build wrote (chunks, copied files) and what plugins add with `emitAsset`.

// ---- Tapable ----------------------------------------------------------------------------------

const tapOptions = (options) => (typeof options === 'string' ? { name: options } : { ...options });

class Hook {
  constructor(args = [], name = undefined, { bail = false, waterfall = false, parallel = false, loop = false, sync = false } = {}) {
    this.args = args;
    this.name = name;
    this.bail = bail;
    this.waterfall = waterfall;
    this.parallel = parallel;
    this.loop = loop;
    this.syncOnly = sync;
    this.taps = [];
    this.interceptors = [];
  }

  _insert(tap) {
    for (const interceptor of this.interceptors) if (interceptor.register) tap = interceptor.register(tap) ?? tap;
    // Tapable's own rule: later taps go after earlier ones of the same stage, `before` pulls one ahead of
    // the named taps, and a lower stage goes first.
    let before = null;
    if (typeof tap.before === 'string') before = new Set([tap.before]);
    else if (Array.isArray(tap.before)) before = new Set(tap.before);
    const stage = typeof tap.stage === 'number' ? tap.stage : 0;
    let i = this.taps.length;
    while (i > 0) {
      i--;
      const other = this.taps[i];
      this.taps[i + 1] = other;
      const otherStage = other.stage || 0;
      if (before) {
        if (before.has(other.name)) {
          before.delete(other.name);
          continue;
        }
        if (before.size > 0) continue;
      }
      if (otherStage > stage) continue;
      i++;
      break;
    }
    this.taps[i] = tap;
  }

  tap(options, fn) {
    this._insert({ ...tapOptions(options), type: 'sync', fn });
  }

  tapAsync(options, fn) {
    if (this.syncOnly) throw new Error(`Cannot tapAsync on the sync hook ${this.name ?? ''}`);
    this._insert({ ...tapOptions(options), type: 'async', fn });
  }

  tapPromise(options, fn) {
    if (this.syncOnly) throw new Error(`Cannot tapPromise on the sync hook ${this.name ?? ''}`);
    this._insert({ ...tapOptions(options), type: 'promise', fn });
  }

  intercept(interceptor) {
    this.interceptors.push(interceptor);
    if (interceptor.register) this.taps = this.taps.map((tap) => interceptor.register(tap) ?? tap);
  }

  isUsed() {
    return this.taps.length > 0;
  }

  withOptions(base) {
    const merge = (options) => ({ ...tapOptions(base), ...tapOptions(options) });
    return {
      name: this.name,
      tap: (options, fn) => this.tap(merge(options), fn),
      tapAsync: (options, fn) => this.tapAsync(merge(options), fn),
      tapPromise: (options, fn) => this.tapPromise(merge(options), fn),
      intercept: (i) => this.intercept(i),
      isUsed: () => this.isUsed(),
      withOptions: (o) => this.withOptions(merge(o)),
    };
  }

  /** Runs a tap; resolves with what it returned. */
  _invoke(tap, args) {
    for (const interceptor of this.interceptors) if (interceptor.tap) interceptor.tap(tap);
    if (tap.type === 'sync') return Promise.resolve().then(() => tap.fn(...args));
    if (tap.type === 'promise') return Promise.resolve().then(() => tap.fn(...args));
    return new Promise((resolve, reject) => tap.fn(...args, (error, result) => (error ? reject(error) : resolve(result))));
  }

  async _run(args) {
    for (const interceptor of this.interceptors) if (interceptor.call) interceptor.call(...args);
    let current = args.slice();
    if (this.parallel) {
      if (this.bail) {
        for (const tap of this.taps) {
          const result = await this._invoke(tap, current);
          if (result !== undefined) return result;
        }
        return undefined;
      }
      await Promise.all(this.taps.map((tap) => this._invoke(tap, current)));
      return undefined;
    }
    do {
      let again = false;
      for (const tap of this.taps) {
        const result = await this._invoke(tap, current);
        if (this.loop) {
          if (result !== undefined) {
            again = true;
            break;
          }
        } else if (this.bail) {
          if (result !== undefined) return result;
        } else if (this.waterfall && result !== undefined) current[0] = result;
      }
      if (!again) return this.waterfall ? current[0] : undefined;
    } while (true);
  }

  /** The synchronous run, for hooks that only have sync taps. */
  _runSync(args) {
    for (const interceptor of this.interceptors) if (interceptor.call) interceptor.call(...args);
    const current = args.slice();
    do {
      let again = false;
      for (const tap of this.taps) {
        if (tap.type !== 'sync') throw new Error(`Cannot call the sync hook ${this.name ?? ''} when a tap (${tap.name}) is async`);
        for (const interceptor of this.interceptors) if (interceptor.tap) interceptor.tap(tap);
        const result = tap.fn(...current);
        if (this.loop) {
          if (result !== undefined) {
            again = true;
            break;
          }
        } else if (this.bail) {
          if (result !== undefined) return result;
        } else if (this.waterfall && result !== undefined) current[0] = result;
      }
      if (!again) return this.waterfall ? current[0] : undefined;
    } while (true);
  }

  call(...args) {
    if (!this.syncOnly) {
      // An async hook has no `call` in webpack; code that does it anyway gets the sync run if it can.
      return this._runSync(args);
    }
    return this._runSync(args);
  }

  callAsync(...args) {
    const callback = args.pop();
    this._run(args).then((result) => callback(null, result), (error) => callback(error));
  }

  promise(...args) {
    return this._run(args);
  }
}

class SyncHook extends Hook {
  constructor(args, name) {
    super(args, name, { sync: true });
  }
}
class SyncBailHook extends Hook {
  constructor(args, name) {
    super(args, name, { sync: true, bail: true });
  }
}
class SyncWaterfallHook extends Hook {
  constructor(args, name) {
    super(args, name, { sync: true, waterfall: true });
  }
}
class SyncLoopHook extends Hook {
  constructor(args, name) {
    super(args, name, { sync: true, loop: true });
  }
}
class AsyncParallelHook extends Hook {
  constructor(args, name) {
    super(args, name, { parallel: true });
  }
}
class AsyncParallelBailHook extends Hook {
  constructor(args, name) {
    super(args, name, { parallel: true, bail: true });
  }
}
class AsyncSeriesHook extends Hook {
  constructor(args, name) {
    super(args, name);
  }
}
class AsyncSeriesBailHook extends Hook {
  constructor(args, name) {
    super(args, name, { bail: true });
  }
}
class AsyncSeriesWaterfallHook extends Hook {
  constructor(args, name) {
    super(args, name, { waterfall: true });
  }
}
class AsyncSeriesLoopHook extends Hook {
  constructor(args, name) {
    super(args, name, { loop: true });
  }
}

class HookMap {
  constructor(factory, name) {
    this._map = new Map();
    this._factory = factory;
    this.name = name;
    this._interceptors = [];
  }
  get(key) {
    return this._map.get(key);
  }
  for(key) {
    let hook = this._map.get(key);
    if (hook) return hook;
    hook = this._factory(key);
    for (const interceptor of this._interceptors) if (interceptor.factory) hook = interceptor.factory(key, hook) ?? hook;
    this._map.set(key, hook);
    return hook;
  }
  tap(key, options, fn) {
    return this.for(key).tap(options, fn);
  }
  tapAsync(key, options, fn) {
    return this.for(key).tapAsync(options, fn);
  }
  tapPromise(key, options, fn) {
    return this.for(key).tapPromise(options, fn);
  }
  intercept(interceptor) {
    this._interceptors.push(interceptor);
  }
  isUsed() {
    return [...this._map.values()].some((h) => h.isUsed());
  }
}

class MultiHook {
  constructor(hooks, name) {
    this.hooks = hooks;
    this.name = name;
  }
  tap(options, fn) {
    for (const hook of this.hooks) hook.tap(options, fn);
  }
  tapAsync(options, fn) {
    for (const hook of this.hooks) hook.tapAsync(options, fn);
  }
  tapPromise(options, fn) {
    for (const hook of this.hooks) hook.tapPromise(options, fn);
  }
  intercept(interceptor) {
    for (const hook of this.hooks) hook.intercept(interceptor);
  }
  isUsed() {
    return this.hooks.some((h) => h.isUsed());
  }
}

const tapable = { SyncHook, SyncBailHook, SyncWaterfallHook, SyncLoopHook, AsyncParallelHook, AsyncParallelBailHook, AsyncSeriesHook, AsyncSeriesBailHook, AsyncSeriesWaterfallHook, AsyncSeriesLoopHook, HookMap, MultiHook };

// ---- webpack-sources --------------------------------------------------------------------------

const textOf = (value) => (typeof value === 'string' ? value : Buffer.isBuffer(value) ? value.toString('utf8') : String(value));

class Source {
  source() {
    throw new Error('Abstract');
  }
  buffer() {
    const value = this.source();
    return Buffer.isBuffer(value) ? value : Buffer.from(value, 'utf8');
  }
  size() {
    return this.buffer().length;
  }
  map() {
    return null;
  }
  sourceAndMap() {
    return { source: this.source(), map: this.map() };
  }
  updateHash(hash) {
    hash.update(this.buffer());
  }
}

class RawSource extends Source {
  constructor(value, convertToString = false) {
    super();
    this._value = convertToString && Buffer.isBuffer(value) ? value.toString('utf8') : value;
  }
  source() {
    return this._value;
  }
  isBuffer() {
    return Buffer.isBuffer(this._value);
  }
}

class OriginalSource extends Source {
  constructor(value, name) {
    super();
    this._value = textOf(value);
    this._name = name;
  }
  source() {
    return this._value;
  }
  getName() {
    return this._name;
  }
  map() {
    return null;
  }
}

class SourceMapSource extends Source {
  constructor(value, name, sourceMap, originalSource, innerSourceMap, removeOriginalSource) {
    super();
    this._value = textOf(value);
    this._name = name;
    this._sourceMap = typeof sourceMap === 'string' ? JSON.parse(sourceMap) : Buffer.isBuffer(sourceMap) ? JSON.parse(sourceMap.toString('utf8')) : sourceMap;
    this._originalSource = originalSource;
    this._innerSourceMap = innerSourceMap;
  }
  source() {
    return this._value;
  }
  map() {
    return this._sourceMap ?? null;
  }
  getArgsAsBuffers() {
    return [Buffer.from(this._value), this._name, Buffer.from(JSON.stringify(this._sourceMap ?? null)), this._originalSource, this._innerSourceMap];
  }
}

class ConcatSource extends Source {
  constructor(...items) {
    super();
    this._children = [];
    for (const item of items) this.add(item);
  }
  add(item) {
    if (item instanceof ConcatSource) this._children.push(...item._children);
    else this._children.push(item);
  }
  addAllSkipOptimizing(items) {
    for (const item of items) this._children.push(item);
  }
  getChildren() {
    return this._children;
  }
  source() {
    return this._children.map((c) => (typeof c === 'string' ? c : textOf(c.source()))).join('');
  }
  map() {
    return null;
  }
}

class PrefixSource extends Source {
  constructor(prefix, source) {
    super();
    this._prefix = prefix;
    this._source = typeof source === 'string' || Buffer.isBuffer(source) ? new RawSource(source) : source;
  }
  source() {
    const text = textOf(this._source.source());
    return this._prefix + text.replace(/\n(?=[^\n])/g, '\n' + this._prefix);
  }
  getPrefix() {
    return this._prefix;
  }
  original() {
    return this._source;
  }
}

class ReplaceSource extends Source {
  constructor(source, name) {
    super();
    this._source = source;
    this._name = name;
    this._replacements = [];
  }
  replace(start, end, content, name) {
    this._replacements.push({ start, end, content, name });
  }
  insert(pos, content, name) {
    this._replacements.push({ start: pos, end: pos - 1, content, name });
  }
  getName() {
    return this._name;
  }
  original() {
    return this._source;
  }
  getReplacements() {
    return this._replacements;
  }
  source() {
    const original = textOf(this._source.source());
    const list = this._replacements.map((r, i) => ({ ...r, i })).sort((a, b) => a.start - b.start || a.end - b.end || a.i - b.i);
    let out = '';
    let at = 0;
    for (const r of list) {
      out += original.slice(at, Math.max(at, r.start)) + r.content;
      at = Math.max(at, r.end + 1);
    }
    return out + original.slice(at);
  }
}

class CachedSource extends Source {
  constructor(source) {
    super();
    this._original = source;
  }
  source() {
    return this._original.source();
  }
  map(options) {
    return this._original.map(options);
  }
  original() {
    return this._original;
  }
}

class SizeOnlySource extends Source {
  constructor(size) {
    super();
    this._size = size;
  }
  size() {
    return this._size;
  }
  source() {
    throw new Error('Content and Map of this Source is not available (only size() is supported)');
  }
}

class CompatSource extends Source {
  constructor(source) {
    super();
    this._source = source;
  }
  static from(source) {
    return source instanceof Source ? source : new CompatSource(source);
  }
  source() {
    return this._source.source();
  }
  map(options) {
    return this._source.map?.(options) ?? null;
  }
}

const webpackSources = { Source, RawSource, OriginalSource, SourceMapSource, ConcatSource, PrefixSource, ReplaceSource, CachedSource, SizeOnlySource, CompatSource };

/** The text of any source object (ours, or one made by a real webpack-sources). */
const sourceText = (source) => {
  if (typeof source === 'string') return source;
  if (Buffer.isBuffer(source)) return source.toString('utf8');
  const value = source.source();
  return textOf(value);
};
const sourceBytes = (source) => {
  if (typeof source === 'string') return Buffer.from(source, 'utf8');
  if (Buffer.isBuffer(source)) return source;
  const value = source.source();
  return Buffer.isBuffer(value) ? value : Buffer.from(value, 'utf8');
};

// ---- naming: [name], [hash:8], [contenthash] ... ----------------------------------------------

const hashText = (text, length) => crypto.createHash('sha256').update(text).digest('hex').slice(0, length ?? 64);

/** webpack's filename templates, filled in from `data` ({ filename, hash, chunk, contentHash, ... }). */
const fillTemplate = (template, data = {}) =>
  String(template).replace(/\[([a-zA-Z]+)(?::(\d+))?\]/g, (whole, key, length) => {
    const cut = (value) => (value === undefined || value === null ? '' : length ? String(value).slice(0, Number(length)) : String(value));
    const filename = data.filename ?? '';
    const file = filename.split(/[?#]/)[0];
    switch (key) {
      case 'file': return file;
      case 'query': return filename.includes('?') ? '?' + filename.split('?')[1].split('#')[0] : '';
      case 'fragment': return filename.includes('#') ? '#' + filename.split('#')[1] : '';
      case 'path': return path.posix.dirname(file) === '.' ? '' : path.posix.dirname(file) + '/';
      case 'base': return path.posix.basename(file);
      case 'name': return data.chunk?.name ?? path.posix.basename(file, path.posix.extname(file));
      case 'ext': return path.posix.extname(file);
      case 'id': return cut(data.chunk?.id ?? data.module?.id ?? '');
      case 'hash':
      case 'fullhash': return cut(data.hash ?? data.chunk?.hash ?? '');
      case 'chunkhash': return cut(data.chunk?.hash ?? data.hash ?? '');
      case 'contenthash': return cut(data.contentHash ?? data.chunk?.contentHash?.javascript ?? data.hash ?? '');
      case 'runtime': return data.runtime ?? data.chunk?.runtime ?? '';
      default: return whole;
    }
  });

// ---- a small LazySet and the loggers ----------------------------------------------------------

class LazySet extends Set {
  addAll(iterable) {
    for (const item of iterable) this.add(item);
    return this;
  }
}

const quietLogger = (origin) => ({
  error: (...a) => log('error', `[${origin}] ${a.join(' ')}`),
  warn: (...a) => log('warn', `[${origin}] ${a.join(' ')}`),
  info() {},
  log() {},
  debug() {},
  trace() {},
  group() {},
  groupCollapsed() {},
  groupEnd() {},
  status() {},
  clear() {},
  profile() {},
  profileEnd() {},
  time() {},
  timeEnd() {},
  timeLog() {},
  timeAggregate() {},
  timeAggregateEnd() {},
  assert() {},
});

class WebpackError extends Error {
  constructor(message) {
    super(message);
    this.name = 'WebpackError';
    this.details = undefined;
    this.module = undefined;
    this.loc = undefined;
    this.hideStack = undefined;
    this.chunk = undefined;
    this.file = undefined;
  }
}

// ---- modules and chunks as plugins see them ---------------------------------------------------

const relativeId = (id) => (path.isAbsolute(id) ? path.relative(root, id).split(path.sep).join('/') || id : id.replace(/^\0/, ''));

class NormalModule {
  constructor(id, info = {}) {
    this.type = 'javascript/auto';
    this.resource = id;
    this.resourceResolveData = { path: id.split('?')[0], query: id.includes('?') ? '?' + id.split('?')[1] : '', fragment: '' };
    this.request = id;
    this.userRequest = id;
    this.rawRequest = relativeId(id);
    this.matchResource = undefined;
    this.context = path.dirname(id.split('?')[0]);
    this.loaders = [];
    this.parser = {};
    this.generator = {};
    this.buildInfo = { fileDependencies: new LazySet([id]), contextDependencies: new LazySet(), missingDependencies: new LazySet(), buildDependencies: new LazySet(), assets: undefined, assetsInfo: undefined };
    this.buildMeta = { exportsType: info.esm ? 'namespace' : 'dynamic' };
    this.dependencies = [];
    this.blocks = [];
    this.presentationalDependencies = [];
    this.warnings = [];
    this.errors = [];
    this.id = relativeId(id);
    this.debugId = this.id;
    this.useSourceMap = false;
    this.useSimpleSourceMap = false;
    this._info = info;
    this._code = info.code ?? null;
    this.optional = false;
    this.factoryMeta = {};
    this.buildMeta.sideEffectFree = info.sideEffects === false;
  }
  identifier() {
    return this.resource;
  }
  readableIdentifier(requestShortener) {
    return typeof requestShortener?.shorten === 'function' ? requestShortener.shorten(this.resource) : relativeId(this.resource);
  }
  libIdent() {
    return './' + relativeId(this.resource);
  }
  nameForCondition() {
    return this.resource.split('?')[0];
  }
  originalSource() {
    return this._code === null ? null : new RawSource(this._code);
  }
  size() {
    return this._code?.length ?? 0;
  }
  getSourceTypes() {
    return new Set(['javascript']);
  }
  getNumberOfErrors() {
    return this.errors.length;
  }
  getNumberOfWarnings() {
    return this.warnings.length;
  }
  addWarning(w) {
    this.warnings.push(w);
  }
  addError(e) {
    this.errors.push(e);
  }
  addDependency(d) {
    this.dependencies.push(d);
  }
  isModule() {
    return true;
  }
  get hot() {
    return false;
  }
  updateHash(hash) {
    hash.update(this._code ?? this.resource);
  }
  static getCompilationHooks(compilation) {
    return (compilation._normalModuleHooks ??= { loader: new SyncHook(['loaderContext', 'module']), beforeLoaders: new SyncHook(['loaders', 'module', 'loaderContext']), beforeParse: new SyncHook(['module']), beforeSnapshot: new SyncHook(['module']), readResourceForScheme: new HookMap(() => new AsyncSeriesBailHook(['resource', 'module']), 'readResourceForScheme'), readResource: new HookMap(() => new AsyncSeriesBailHook(['loaderContext']), 'readResource'), needBuild: new AsyncSeriesBailHook(['module', 'context']) });
  }
}

class Chunk {
  constructor(name, id) {
    this.id = id;
    this.ids = [id];
    this.name = name;
    this.debugId = id;
    this.idNameHints = new Set(name ? [name] : []);
    this.preventIntegration = false;
    this.filenameTemplate = undefined;
    this.cssFilenameTemplate = undefined;
    this.files = new Set();
    this.auxiliaryFiles = new Set();
    this.rendered = true;
    this.hash = undefined;
    this.contentHash = Object.create(null);
    this.renderedHash = undefined;
    this.chunkReason = undefined;
    this.extraAsync = false;
    this.runtime = name;
    this._modules = [];
    this._initial = true;
    this._entry = true;
    this._groups = new Set();
  }
  hasRuntime() {
    return this._entry;
  }
  canBeInitial() {
    return this._initial;
  }
  isOnlyInitial() {
    return this._initial;
  }
  getModules() {
    return this._modules.slice();
  }
  get modulesIterable() {
    return this._modules;
  }
  getNumberOfModules() {
    return this._modules.length;
  }
  get groupsIterable() {
    return this._groups;
  }
  getAllAsyncChunks() {
    return new Set();
  }
  getAllInitialChunks() {
    return new Set([this]);
  }
  getAllReferencedChunks() {
    return new Set([this]);
  }
  containsModule(module) {
    return this._modules.includes(module);
  }
  updateHash(hash) {
    hash.update(`${this.id} ${this.hash ?? ''}`);
  }
  getChunkMaps() {
    return { hash: {}, contentHash: {}, name: {} };
  }
}

class ChunkGroup {
  constructor(name, chunks) {
    this.name = name;
    this.chunks = chunks;
    this.options = { name };
    this.id = name;
    this.index = 0;
    this.origins = [];
  }
  getFiles() {
    const files = [];
    for (const chunk of this.chunks) for (const file of chunk.files) if (!files.includes(file)) files.push(file);
    return files;
  }
  getEntrypointChunk() {
    return this.chunks[this.chunks.length - 1];
  }
  getRuntimeChunk() {
    return this.chunks[this.chunks.length - 1];
  }
  getParents() {
    return [];
  }
  getChildren() {
    return [];
  }
  isInitial() {
    return true;
  }
}

class Entrypoint extends ChunkGroup {}

// ---- cache, stats -----------------------------------------------------------------------------

const noCache = () => {
  const item = {
    get: (cb) => cb(null, undefined),
    getPromise: async () => undefined,
    store: (value, cb) => cb?.(null),
    storePromise: async () => {},
  };
  return {
    get: (identifier, etag, cb) => cb(null, undefined),
    getPromise: async () => undefined,
    store: (identifier, etag, data, cb) => cb?.(null),
    storePromise: async () => {},
    provide: (identifier, etag, computer, cb) => computer(cb),
    providePromise: async (identifier, etag, computer) => computer(),
    getItemCache: () => item,
    getLazyHashedEtag: (object) => `etag:${hashText(JSON.stringify(object?.toString?.() ?? String(object)), 16)}`,
    mergeEtags: (a, b) => `${a}|${b}`,
  };
};

class Stats {
  constructor(compilation) {
    this.compilation = compilation;
  }
  get hash() {
    return this.compilation.hash;
  }
  get startTime() {
    return this.compilation.startTime;
  }
  get endTime() {
    return this.compilation.endTime;
  }
  hasWarnings() {
    return this.compilation.warnings.length > 0;
  }
  hasErrors() {
    return this.compilation.errors.length > 0;
  }
  toJson(options = {}) {
    const c = this.compilation;
    const wanted = (name, fallback = true) => (typeof options === 'object' && options && name in options ? !!options[name] : options === 'none' ? false : typeof options === 'object' && options?.all === false ? false : fallback);
    const assetEntries = Object.entries(c.assets);
    const chunkList = [...c.chunks];
    const idOfFile = (file) => chunkList.filter((k) => k.files.has(file) || k.auxiliaryFiles.has(file)).map((k) => k.id);
    const json = { version: c.compiler.webpack.version, hash: c.hash, time: (c.endTime ?? Date.now()) - (c.startTime ?? Date.now()), builtAt: c.endTime ?? Date.now(), publicPath: c.outputOptions.publicPath === 'auto' ? '' : c.outputOptions.publicPath ?? '', outputPath: c.outputOptions.path };
    const assetOf = (name, source) => ({ type: 'asset', name, size: sourceBytes(source).length, emitted: true, comparedForEmit: false, cached: false, info: c.assetsInfo.get(name) ?? {}, chunkNames: chunkList.filter((k) => k.files.has(name)).map((k) => k.name).filter(Boolean), chunkIdHints: [], auxiliaryChunkNames: [], auxiliaryChunkIdHints: [], related: [], chunks: idOfFile(name), auxiliaryChunks: [] });
    if (wanted('assets')) json.assets = assetEntries.map(([name, source]) => assetOf(name, source));
    json.assetsByChunkName = {};
    for (const k of chunkList) if (k.name) json.assetsByChunkName[k.name] = [...k.files];
    if (wanted('chunks')) {
      json.chunks = chunkList.map((k) => ({ rendered: true, initial: k.canBeInitial(), entry: k.hasRuntime(), recorded: false, reason: undefined, size: [...k.files].reduce((n, f) => n + (c.assets[f] ? sourceBytes(c.assets[f]).length : 0), 0), sizes: {}, names: k.name ? [k.name] : [], idHints: [], runtime: k.name ? [k.name] : [], files: [...k.files], auxiliaryFiles: [...k.auxiliaryFiles], hash: k.hash, childrenByOrder: {}, id: k.id, siblings: [], parents: [], children: [], modules: wanted('modules') ? k.getModules().map((m) => moduleJson(c, m)) : undefined, origins: [] }));
    }
    if (wanted('modules')) json.modules = [...c.modules].map((m) => moduleJson(c, m));
    if (wanted('entrypoints')) {
      json.entrypoints = {};
      for (const [name, group] of c.entrypoints) json.entrypoints[name] = { name, chunks: group.chunks.map((k) => k.id), assets: group.getFiles().map((f) => ({ name: f, size: c.assets[f] ? sourceBytes(c.assets[f]).length : 0 })), filteredAssets: 0, assetsSize: group.getFiles().reduce((n, f) => n + (c.assets[f] ? sourceBytes(c.assets[f]).length : 0), 0), auxiliaryAssets: [], children: {}, childAssets: {}, isOverSizeLimit: false };
    }
    if (wanted('namedChunkGroups')) json.namedChunkGroups = json.entrypoints ?? {};
    json.errors = c.errors.map((e) => ({ message: e?.message ?? String(e), moduleIdentifier: e?.module?.identifier?.(), moduleName: e?.module?.readableIdentifier?.(), loc: e?.loc, details: e?.details, stack: e?.stack }));
    json.errorsCount = c.errors.length;
    json.warnings = c.warnings.map((e) => ({ message: e?.message ?? String(e), moduleIdentifier: e?.module?.identifier?.(), moduleName: e?.module?.readableIdentifier?.(), loc: e?.loc, details: e?.details }));
    json.warningsCount = c.warnings.length;
    json.children = [];
    return json;
  }
  toString() {
    const json = this.toJson();
    return [`Hash: ${json.hash}`, ...json.assets.map((a) => `${a.name}  ${a.size} bytes`), ...json.errors.map((e) => `ERROR: ${e.message}`), ...json.warnings.map((e) => `WARNING: ${e.message}`)].join('\n');
  }
}

const moduleJson = (c, m) => ({ type: 'module', moduleType: m.type, layer: undefined, identifier: m.identifier(), name: m.readableIdentifier(), nameForCondition: m.nameForCondition(), index: 0, preOrderIndex: 0, index2: 0, postOrderIndex: 0, size: m.size(), sizes: { javascript: m.size() }, cacheable: true, built: true, codeGenerated: true, cached: false, optional: false, orphan: false, issuer: null, issuerName: null, issuerPath: null, failed: false, errors: 0, warnings: 0, id: m.id, issuerId: null, chunks: [...c.chunks].filter((k) => k.containsModule(m)).map((k) => k.id), assets: [], reasons: [], usedExports: null, providedExports: null, optimizationBailout: [], depth: 0 });

// ---- the compilation --------------------------------------------------------------------------

const sameBytes = (a, b) => sourceBytes(a).equals(sourceBytes(b));

class ModuleGraphShim {
  constructor(compilation) {
    this.compilation = compilation;
  }
  _byId(id) {
    return this.compilation._modulesById.get(id);
  }
  getModule(dependency) {
    return dependency?.module ?? null;
  }
  getResolvedModule(dependency) {
    return dependency?.module ?? null;
  }
  getIssuer(module) {
    const first = module._info?.importers?.[0];
    return first ? this._byId(first) ?? null : null;
  }
  getParentModule() {
    return null;
  }
  getParentBlock() {
    return null;
  }
  getOutgoingConnections(module) {
    return (module._info?.importedIds ?? []).map((id) => ({ originModule: module, module: this._byId(id) ?? null, dependency: { request: id, type: 'esm import', module: this._byId(id) ?? null }, active: true, conditional: false, explanations: new Set() })).filter((c) => c.module);
  }
  getIncomingConnections(module) {
    return (module._info?.importers ?? []).map((id) => ({ originModule: this._byId(id) ?? null, module, dependency: { request: module.resource, type: 'esm import', module }, active: true })).filter((c) => c.originModule);
  }
  isAsync() {
    return false;
  }
  getMeta() {
    return {};
  }
  getProfile() {
    return undefined;
  }
  getExportsInfo(module) {
    const names = module._info?.exports ?? [];
    return { getProvidedExports: () => names.slice(), isModuleUsed: () => true, isUsed: () => true, getUsedExports: () => names.slice(), otherExportsInfo: { provided: null, getUsed: () => 1 }, getReadOnlyExportInfo: (name) => ({ name, provided: names.includes(name), getUsed: () => 1, used: 1 }) };
  }
  getDepth() {
    return 0;
  }
}

class ChunkGraphShim {
  constructor(compilation) {
    this.compilation = compilation;
  }
  getChunkModulesIterable(chunk) {
    return chunk.getModules();
  }
  getChunkModules(chunk) {
    return chunk.getModules();
  }
  getOrderedChunkModulesIterable(chunk, compare) {
    return chunk.getModules().sort(compare);
  }
  getOrderedChunkModules(chunk, compare) {
    return chunk.getModules().sort(compare);
  }
  getNumberOfChunkModules(chunk) {
    return chunk.getNumberOfModules();
  }
  getModuleChunks(module) {
    return [...this.compilation.chunks].filter((c) => c.containsModule(module));
  }
  getModuleChunksIterable(module) {
    return this.getModuleChunks(module);
  }
  getNumberOfModuleChunks(module) {
    return this.getModuleChunks(module).length;
  }
  getChunkEntryModulesIterable(chunk) {
    return chunk._entryModule ? [chunk._entryModule] : [];
  }
  getChunkEntryDependentChunksIterable() {
    return [];
  }
  hasChunkEntryDependentChunks() {
    return false;
  }
  isEntryModule(module) {
    return [...this.compilation.chunks].some((c) => c._entryModule === module);
  }
  getModuleId(module) {
    return module.id;
  }
  getModuleHash(module) {
    return hashText(module._code ?? module.resource, 16);
  }
  getRenderedModuleHash(module) {
    return this.getModuleHash(module);
  }
  getChunkModuleSourceTypes() {
    return new Set(['javascript']);
  }
  getChunkConditionMap() {
    return {};
  }
  getModuleRuntimes() {
    return new Set([...this.compilation.chunks].map((c) => c.runtime));
  }
  getChunkRootModules(chunk) {
    return chunk.getModules();
  }
  getTreeRuntimeRequirements() {
    return new Set();
  }
  getChunkRuntimeRequirements() {
    return new Set();
  }
}

const STAGES = { ADDITIONAL: -2000, PRE_PROCESS: -1000, DERIVED: -200, ADDITIONS: -100, OPTIMIZE: 100, OPTIMIZE_COUNT: 200, OPTIMIZE_COMPATIBILITY: 300, OPTIMIZE_SIZE: 400, DEV_TOOLING: 500, OPTIMIZE_INLINE: 700, SUMMARIZE: 1000, OPTIMIZE_HASH: 2500, OPTIMIZE_TRANSFER: 3000, ANALYSE: 4000, REPORT: 5000 };

const compilationHookTable = () => {
  const sync = (args = []) => new SyncHook(args);
  const bail = (args = []) => new SyncBailHook(args);
  const series = (args = []) => new AsyncSeriesHook(args);
  const map = (factory) => new HookMap(factory);
  return {
    buildModule: sync(['module']), rebuildModule: sync(['module']), failedModule: sync(['module', 'error']), succeedModule: sync(['module']), stillValidModule: sync(['module']),
    addEntry: sync(['entry', 'options']), failedEntry: sync(['entry', 'options', 'error']), succeedEntry: sync(['entry', 'options', 'module']),
    dependencyReferencedExports: new SyncWaterfallHook(['referencedExports', 'dependency', 'runtime']),
    executeModule: sync(['options', 'context']), prepareModuleExecution: new AsyncParallelHook(['options', 'context']),
    finishModules: series(['modules']), finishRebuildingModule: series(['module']),
    unseal: sync(), seal: sync(), beforeChunks: sync(), afterChunks: sync(['chunks']),
    optimizeDependencies: bail(['modules']), afterOptimizeDependencies: sync(['modules']), optimize: sync(),
    optimizeModules: bail(['modules']), afterOptimizeModules: sync(['modules']),
    optimizeChunks: bail(['chunks', 'chunkGroups']), afterOptimizeChunks: sync(['chunks', 'chunkGroups']),
    optimizeTree: series(['chunks', 'modules']), afterOptimizeTree: sync(['chunks', 'modules']),
    optimizeChunkModules: new AsyncSeriesBailHook(['chunks', 'modules']), afterOptimizeChunkModules: sync(['chunks', 'modules']),
    shouldRecord: bail(), additionalChunkRuntimeRequirements: sync(['chunk', 'runtimeRequirements', 'context']),
    runtimeRequirementInChunk: map(() => bail(['chunk', 'runtimeRequirements', 'context'])), additionalModuleRuntimeRequirements: sync(['module', 'runtimeRequirements', 'context']),
    runtimeRequirementInModule: map(() => bail(['module', 'runtimeRequirements', 'context'])), additionalTreeRuntimeRequirements: sync(['chunk', 'runtimeRequirements', 'context']),
    runtimeRequirementInTree: map(() => bail(['chunk', 'runtimeRequirements', 'context'])), runtimeModule: sync(['module', 'chunk']),
    reviveModules: sync(['modules', 'records']), beforeModuleIds: sync(['modules']), moduleIds: sync(['modules']), optimizeModuleIds: sync(['modules']), afterOptimizeModuleIds: sync(['modules']),
    reviveChunks: sync(['chunks', 'records']), beforeChunkIds: sync(['chunks']), chunkIds: sync(['chunks']), optimizeChunkIds: sync(['chunks']), afterOptimizeChunkIds: sync(['chunks']),
    recordModules: sync(['modules', 'records']), recordChunks: sync(['chunks', 'records']), optimizeCodeGeneration: sync(['modules']),
    beforeModuleHash: sync(), afterModuleHash: sync(), beforeCodeGeneration: sync(), afterCodeGeneration: sync(), beforeRuntimeRequirements: sync(), afterRuntimeRequirements: sync(),
    beforeHash: sync(), contentHash: sync(['chunk']), afterHash: sync(), recordHash: sync(['records']), record: sync(['compilation', 'records']),
    beforeModuleAssets: sync(), shouldGenerateChunkAssets: bail(), beforeChunkAssets: sync(), additionalChunkAssets: sync(['chunks']), additionalAssets: series(), optimizeChunkAssets: series(['chunks']), afterOptimizeChunkAssets: sync(['chunks']),
    optimizeAssets: series(['assets']), afterOptimizeAssets: sync(['assets']), processAssets: series(['assets']), afterProcessAssets: sync(['assets']), processAdditionalAssets: series(['assets']),
    needAdditionalSeal: bail(), afterSeal: series(), renderManifest: new SyncWaterfallHook(['result', 'options']), fullHash: sync(['hash']), chunkHash: sync(['chunk', 'chunkHash', 'ssr']),
    moduleAsset: sync(['module', 'filename']), chunkAsset: sync(['chunk', 'filename']), assetPath: new SyncWaterfallHook(['path', 'options', 'assetInfo']), needAdditionalPass: bail(),
    childCompiler: sync(['childCompiler', 'compilerName', 'compilerIndex']), log: bail(['origin', 'logEntry']), processWarnings: new SyncWaterfallHook(['warnings']), processErrors: new SyncWaterfallHook(['errors']),
    statsPreset: map(() => sync(['options', 'context'])), statsNormalize: sync(['options', 'context']), statsFactory: sync(['statsFactory', 'options']), statsPrinter: sync(['statsPrinter', 'options']),
  };
};

/** Hooks nobody here knows about are accepted and never called, so a plugin that taps one does not fail. */
const tolerant = (table) =>
  new Proxy(table, {
    get(target, name) {
      if (typeof name !== 'string' || name in target) return target[name];
      return (target[name] = new AsyncSeriesHook([], name));
    },
  });

class Compilation {
  constructor(compiler, params) {
    this.compiler = compiler;
    this.options = compiler.options;
    this.outputOptions = compiler.options.output;
    this.inputFileSystem = compiler.inputFileSystem;
    this.outputFileSystem = compiler.outputFileSystem;
    this.intermediateFileSystem = compiler.intermediateFileSystem;
    this.resolverFactory = compiler.resolverFactory;
    this.requestShortener = compiler.requestShortener;
    this.params = params;
    this.hooks = tolerant(compilationHookTable());
    this.name = undefined;
    this.startTime = Date.now();
    this.endTime = undefined;
    this.hash = undefined;
    this.fullHash = undefined;
    this.assets = {};
    this.assetsInfo = new Map();
    this.errors = [];
    this.warnings = [];
    this.children = [];
    this.logging = new Map();
    this.modules = new Set();
    this._modulesById = new Map();
    this.chunks = new Set();
    this.chunkGroups = [];
    this.namedChunks = new Map();
    this.namedChunkGroups = new Map();
    this.entrypoints = new Map();
    this.asyncEntrypoints = [];
    this.entries = new Map();
    this.fileDependencies = new LazySet();
    this.contextDependencies = new LazySet();
    this.missingDependencies = new LazySet();
    this.buildDependencies = new LazySet();
    this.compilationDependencies = { add: (f) => this.fileDependencies.add(f) };
    this.emittedAssets = new Set();
    this.comparedForEmitAssets = new Set();
    this.dependencyFactories = new Map();
    this.dependencyTemplates = new Map();
    this.childrenCounters = {};
    this.usedChunkIds = null;
    this.usedModuleIds = null;
    this.needAdditionalPass = false;
    this.builtModules = new WeakSet();
    this.codeGeneratedModules = new WeakSet();
    this.buildTimeExecutedModules = new WeakSet();
    this.moduleGraph = new ModuleGraphShim(this);
    this.chunkGraph = new ChunkGraphShim(this);
    this.codeGenerationResults = { get: () => ({ sources: new Map() }), has: () => false };
    this.mainTemplate = { hooks: tolerant({}) };
    this.chunkTemplate = { hooks: tolerant({}) };
    this.runtimeTemplate = { supportsConst: () => true, supportsArrowFunction: () => true, outputOptions: this.outputOptions, requestShortener: this.requestShortener, basicFunction: (args, body) => `function(${args}) {\n${Array.isArray(body) ? body.join('\n') : body}\n}` };
    this.moduleTemplates = { javascript: { hooks: tolerant({}) } };
    this.records = null;
    this.valueCacheVersions = new Map();
    this.fileSystemInfo = { createSnapshot: (start, files, dirs, missing, options, callback) => callback(null, { files, dirs, missing }), checkSnapshotValid: (snapshot, callback) => callback(null, false), mergeSnapshots: (a) => a, getFileTimestamp: (f, cb) => cb(null, null), getContextTimestamp: (f, cb) => cb(null, null), getFileHash: (f, cb) => cb(null, null), addFileTimestamps() {}, addContextTimestamps() {}, logStatistics() {}, createSnapshotSync() {} };
  }

  getLogger(name) {
    return quietLogger(typeof name === 'function' ? name() : name);
  }
  getCache(name) {
    return noCache();
  }
  getStats() {
    return new Stats(this);
  }
  createStatsOptions(options, context = {}) {
    return typeof options === 'object' && options ? { ...options } : { preset: options };
  }
  addModule(module, callback) {
    this.modules.add(module);
    callback?.(null, module);
  }
  addEntry(context, entry, options, callback) {
    callback?.(null, null);
  }
  addInclude(context, dependency, options, callback) {
    callback?.(null, null);
  }
  addChunk(name) {
    if (name && this.namedChunks.has(name)) return this.namedChunks.get(name);
    const chunk = new Chunk(name, name ?? `chunk${this.chunks.size}`);
    this.chunks.add(chunk);
    if (name) this.namedChunks.set(name, chunk);
    return chunk;
  }
  findModule(identifier) {
    return this._modulesById.get(identifier);
  }
  getModule(module) {
    return this._modulesById.get(module.identifier());
  }
  rebuildModule(module, callback) {
    callback(null, module);
  }
  reportDependencyErrorsAndWarnings() {}
  summarizeDependencies() {}

  // ---- assets ----
  _setAssetInfo(file, info) {
    this.assetsInfo.set(file, info === undefined ? this.assetsInfo.get(file) ?? {} : info);
  }
  emitAsset(file, source, assetInfo = {}) {
    if (this.assets[file]) {
      if (!sameBytes(this.assets[file], source)) {
        this.errors.push(new WebpackError(`Conflict: Multiple assets emit different content to the same filename ${file}`));
        this.assets[file] = source;
        this._setAssetInfo(file, assetInfo);
        return;
      }
      this._setAssetInfo(file, { ...(this.assetsInfo.get(file) ?? {}), ...assetInfo });
      return;
    }
    this.assets[file] = source;
    this._setAssetInfo(file, assetInfo);
  }
  updateAsset(file, newSourceOrFunction, assetInfoUpdateOrFunction) {
    if (!(file in this.assets)) throw new Error(`Called Compilation.updateAsset for not existing filename ${file}`);
    this.assets[file] = typeof newSourceOrFunction === 'function' ? newSourceOrFunction(this.assets[file]) : newSourceOrFunction;
    if (assetInfoUpdateOrFunction !== undefined) {
      const old = this.assetsInfo.get(file) ?? {};
      this.assetsInfo.set(file, typeof assetInfoUpdateOrFunction === 'function' ? assetInfoUpdateOrFunction(old) || old : { ...old, ...assetInfoUpdateOrFunction });
    }
  }
  renameAsset(file, newFile) {
    const source = this.assets[file];
    if (!source) throw new Error(`Called Compilation.renameAsset for not existing filename ${file}`);
    if (this.assets[newFile] && !sameBytes(this.assets[newFile], source)) this.errors.push(new WebpackError(`Conflict: Called Compilation.renameAsset for already existing filename ${newFile} with different content`));
    const info = this.assetsInfo.get(file);
    delete this.assets[file];
    this.assetsInfo.delete(file);
    this.assets[newFile] = source;
    this.assetsInfo.set(newFile, info ?? {});
    for (const chunk of this.chunks) {
      for (const set of [chunk.files, chunk.auxiliaryFiles]) {
        if (set.delete(file)) set.add(newFile);
      }
    }
  }
  deleteAsset(file) {
    if (!(file in this.assets)) return;
    delete this.assets[file];
    this.assetsInfo.delete(file);
    for (const chunk of this.chunks) {
      chunk.files.delete(file);
      chunk.auxiliaryFiles.delete(file);
    }
  }
  getAssets() {
    return Object.keys(this.assets).map((name) => ({ name, source: this.assets[name], info: this.assetsInfo.get(name) ?? {} }));
  }
  getAsset(name) {
    return name in this.assets ? { name, source: this.assets[name], info: this.assetsInfo.get(name) ?? {} } : undefined;
  }
  getPath(filename, data = {}) {
    const template = typeof filename === 'function' ? filename(data.pathData ?? data, data.assetInfo) : filename;
    const text = fillTemplate(template, { hash: this.hash, ...data });
    return this.hooks.assetPath.call(text, data, undefined);
  }
  getPathWithInfo(filename, data = {}) {
    return { path: this.getPath(filename, data), info: {} };
  }
  getAssetPath(filename, data) {
    return this.getPath(filename, data);
  }
  getAssetPathWithInfo(filename, data) {
    return this.getPathWithInfo(filename, data);
  }
  createChildCompiler(name, outputOptions, plugins) {
    return this.compiler.createChildCompiler(this, name, this.childrenCounters[name] = (this.childrenCounters[name] ?? -1) + 1, outputOptions, plugins);
  }
}
for (const [name, value] of Object.entries(STAGES)) Compilation[`PROCESS_ASSETS_STAGE_${name}`] = value;

// ---- the compiler -----------------------------------------------------------------------------

const compilerHookTable = () => ({
  environment: new SyncHook([]), afterEnvironment: new SyncHook([]), afterResolvers: new SyncHook(['compiler']),
  entryOption: new SyncBailHook(['context', 'entry']), afterPlugins: new SyncHook(['compiler']), initialize: new SyncHook([]),
  beforeRun: new AsyncSeriesHook(['compiler']), run: new AsyncSeriesHook(['compiler']), watchRun: new AsyncSeriesHook(['compiler']),
  normalModuleFactory: new SyncHook(['normalModuleFactory']), contextModuleFactory: new SyncHook(['contextModuleFactory']),
  beforeCompile: new AsyncSeriesHook(['params']), compile: new SyncHook(['params']),
  thisCompilation: new SyncHook(['compilation', 'params']), compilation: new SyncHook(['compilation', 'params']),
  make: new AsyncParallelHook(['compilation']), finishMake: new AsyncSeriesHook(['compilation']), afterCompile: new AsyncSeriesHook(['compilation']),
  shouldEmit: new SyncBailHook(['compilation']), emit: new AsyncSeriesHook(['compilation']), assetEmitted: new AsyncSeriesHook(['file', 'info']),
  afterEmit: new AsyncSeriesHook(['compilation']), done: new AsyncSeriesHook(['stats']), additionalPass: new AsyncSeriesHook([]),
  failed: new SyncHook(['error']), invalid: new SyncHook(['filename', 'changeTime']), watchClose: new SyncHook([]), shutdown: new AsyncSeriesHook([]),
  infrastructureLog: new SyncBailHook(['origin', 'type', 'args']), readRecords: new AsyncSeriesHook([]), emitRecords: new AsyncSeriesHook([]),
});

const inputFileSystem = Object.assign(Object.create(fs), {
  purge() {},
  join: (...a) => path.join(...a),
  dirname: (p) => path.dirname(p),
  relative: (a, b) => path.relative(a, b),
  readJson: (file, cb) => fs.readFile(file, 'utf8', (e, t) => { if (e) return cb(e); try { cb(null, JSON.parse(t)); } catch (x) { cb(x); } }),
});
const outputFileSystem = Object.assign(Object.create(fs), {
  mkdirp: (dir, cb) => fs.mkdir(dir, { recursive: true }, (e) => cb(e)),
  join: (...a) => path.join(...a),
  dirname: (p) => path.dirname(p),
});

class Compiler {
  constructor(context, options) {
    this.hooks = tolerant(compilerHookTable());
    this.webpack = webpackNamespace;
    this.name = options.name;
    this.parentCompilation = undefined;
    this.root = this;
    this.outputPath = options.output.path;
    this.inputFileSystem = inputFileSystem;
    this.outputFileSystem = outputFileSystem;
    this.intermediateFileSystem = outputFileSystem;
    this.watchFileSystem = null;
    this.records = {};
    this.managedPaths = new Set();
    this.immutablePaths = new Set();
    this.modifiedFiles = undefined;
    this.removedFiles = undefined;
    this.fileTimestamps = new Map();
    this.contextTimestamps = new Map();
    this.fsStartTime = undefined;
    this.options = options;
    this.context = context;
    this.requestShortener = { shorten: (s) => relativeId(String(s)) };
    this.cache = { hooks: { shutdown: new AsyncSeriesHook([]), beginIdle: new SyncHook([]), endIdle: new AsyncSeriesHook([]), storeBuildDependencies: new AsyncSeriesHook(['deps']) }, get: (i, e, cb) => cb(null, undefined), store: (i, e, d, cb) => cb?.(null), beginIdle() {}, endIdle(cb) { cb?.(); }, shutdown(cb) { cb?.(); } };
    this.compilerPath = '';
    this.running = false;
    this.idle = false;
    this.watchMode = false;
    this.resolverFactory = {
      hooks: tolerant({}),
      get: () => {
        const resolver = {
          hooks: tolerant({}),
          resolve: (ctx, base, request, resolveContext, callback) => resolveId(request, path.join(base, 'noop.js'), { kind: 'import-statement' }, null).then((r) => (r ? callback(null, r.id, { path: r.id }) : callback(new Error(`Can't resolve '${request}' in '${base}'`))), callback),
          resolveSync: () => {
            throw new Error('resolveSync is not supported');
          },
          withOptions: () => resolver,
        };
        return resolver;
      },
    };
  }
  getInfrastructureLogger(name) {
    return quietLogger(typeof name === 'function' ? name() : name);
  }
  getCache(name) {
    return noCache();
  }
  isChild() {
    return !!this.parentCompilation;
  }
  watch() {
    throw new Error('compiler.watch() is not available: use rathkit-pack bundle --watch');
  }
  run() {
    throw new Error('compiler.run() is not available: rathkit-pack runs the build');
  }
  close(callback) {
    callback?.();
  }
  purgeInputFileSystem() {}
  createChildCompiler(compilation, compilerName, compilerIndex, outputOptions, plugins) {
    return createChildCompiler(this, compilation, compilerName, compilerIndex, outputOptions, plugins);
  }
}

// ---- child compilers ---------------------------------------------------------------------------

const CHILD_OWN_HOOKS = new Set(['make', 'compile', 'emit', 'afterEmit', 'invalid', 'done', 'thisCompilation']);

/**
 * A compiler for a smaller job inside a build (html-webpack-plugin compiles its template with one).
 * Its entries are built by the bundler itself, as CommonJS, and wrapped in the library variable the
 * plugin asked for.
 */
function createChildCompiler(parent, compilation, name, index, outputOptions, plugins) {
  const options = { ...parent.options, output: { ...parent.options.output, ...outputOptions }, plugins: [], entry: {} };
  const child = new Compiler(parent.context, options);
  child.name = name;
  child.parentCompilation = compilation;
  child.root = parent.root;
  child.outputPath = parent.outputPath;
  child._entries = [];
  for (const hookName of Object.keys(compilerHookTable())) {
    if (!CHILD_OWN_HOOKS.has(hookName)) child.hooks[hookName].taps = parent.hooks[hookName].taps.slice();
  }
  for (const plugin of plugins ?? []) {
    if (typeof plugin === 'function') plugin.call(child, child);
    else plugin.apply(child);
  }
  compilation.hooks.childCompiler.call(child, name, index);
  child.runAsChild = (callback) => {
    runChild(child).then(([entries, childCompilation]) => callback(null, entries, childCompilation), (error) => callback(error));
  };
  return child;
}

async function runChild(child) {
  const params = { normalModuleFactory: makeNormalModuleFactory(), contextModuleFactory: { hooks: tolerant({}) } };
  child.hooks.normalModuleFactory.call(params.normalModuleFactory);
  await child.hooks.beforeCompile.promise(params);
  child.hooks.compile.call(params);
  const compilation = new Compilation(child, params);
  compilation.name = child.name;
  child.hooks.thisCompilation.call(compilation, params);
  child.hooks.compilation.call(compilation, params);
  await child.hooks.make.promise(compilation);
  await child.hooks.finishMake.promise(compilation);
  // Entries with one name make one file; a `data:` entry is a snippet that sets webpack's public path.
  const groups = new Map();
  for (const entry of child._entries) {
    const name = entry.name ?? path.basename(entry.request);
    if (!groups.has(name)) groups.set(name, []);
    groups.get(name).push(entry);
  }
  const library = child.options.output.library;
  const global = typeof library === 'string' ? library : library?.name;
  for (const [name, entries] of groups) {
    const real = entries.filter((e) => !e.request.startsWith('data:'));
    const codes = [];
    for (const entry of real) {
      try {
        const built = await ask('bundleEntry', { entry: entry.request, globalName: global ?? 'RESULT' });
        codes.push(built.code);
        for (const asset of built.assets) compilation.emitAsset(asset.name, new RawSource(Buffer.from(asset.data, 'base64')));
      } catch (error) {
        compilation.errors.push(new WebpackError(`${error.message}`));
      }
    }
    if (compilation.errors.length) continue;
    const prelude = entries.length > real.length ? 'var __webpack_public_path__ = typeof htmlWebpackPluginPublicPath === "string" ? htmlWebpackPluginPublicPath : ""; var __webpack_base_uri__ = __webpack_public_path__;\n' : '';
    const body = codes.join('\n');
    const wrapped = global ? `var ${global} = (function () {\nvar module = { exports: {} };\nvar exports = module.exports;\n${body}\nreturn module.exports;\n})();` : body;
    const chunk = compilation.addChunk(name);
    const fileName = fillTemplate(typeof child.options.output.filename === 'function' ? child.options.output.filename({ chunk }) : child.options.output.filename, { chunk, filename: name });
    compilation.emitAsset(fileName, new RawSource(prelude + wrapped));
    chunk.files.add(fileName);
    const group = new Entrypoint(name, [chunk]);
    compilation.entrypoints.set(name, group);
    compilation.chunkGroups.push(group);
  }
  compilation.hooks.seal.call();
  await compilation.hooks.processAssets.promise(compilation.assets);
  compilation.hooks.afterProcessAssets.call(compilation.assets);
  await child.hooks.afterCompile.promise(compilation);
  const parentCompilation = child.parentCompilation;
  parentCompilation.children.push(compilation);
  for (const { name, source, info } of compilation.getAssets()) parentCompilation.emitAsset(name, source, info);
  return [[...compilation.entrypoints.values()], compilation];
}

// ---- plugins that come with webpack ------------------------------------------------------------

const matchPart = (str, test) => {
  if (test === undefined || test === null) return true;
  if (Array.isArray(test)) return test.some((t) => matchPart(str, t));
  if (typeof test === 'string') return str.startsWith(test);
  if (typeof test === 'function') return !!test(str);
  if (test instanceof RegExp) return ((test.lastIndex = 0), test.test(str));
  return false;
};
const ModuleFilenameHelpers = {
  matchPart,
  matchObject: (obj, str) => {
    if (obj.test && !matchPart(str, obj.test)) return false;
    if (obj.include && !matchPart(str, obj.include)) return false;
    if (obj.exclude && matchPart(str, obj.exclude)) return false;
    return true;
  },
  createFilename: (module, options, { requestShortener }) => (typeof module === 'string' ? module : relativeId(module.resource ?? String(module))),
  replaceDuplicates: (array) => array,
  createFooter: () => '',
};

const wrapComment = (text) => (text.includes('\n') ? `/*!\n * ${text.replace(/\*\//g, '* /').split('\n').join('\n * ')}\n */` : `/*! ${text.replace(/\*\//g, '* /')} */`);

class DefinePlugin {
  constructor(definitions) {
    this.definitions = definitions;
  }
  apply() {
    configOptions.define = Object.fromEntries([...Object.entries(configOptions.define ?? {}), ...definitionsOf(this.definitions)]);
  }
  static runtimeValue(fn, options) {
    return { fn, options, [Symbol.for('rk.runtimeValue')]: true };
  }
}

class EnvironmentPlugin {
  constructor(...keys) {
    if (keys.length === 1 && Array.isArray(keys[0])) {
      this.keys = keys[0];
      this.defaultValues = {};
    } else if (keys[0] && typeof keys[0] === 'object') {
      this.keys = Object.keys(keys[0]);
      this.defaultValues = keys[0];
    } else {
      this.keys = keys;
      this.defaultValues = {};
    }
  }
  apply() {
    const definitions = {};
    for (const key of this.keys) {
      const value = process.env[key] !== undefined ? process.env[key] : this.defaultValues[key];
      if (value === undefined) log('warn', `EnvironmentPlugin - ${key} environment variable is undefined.`);
      definitions[`process.env.${key}`] = value === undefined ? 'undefined' : JSON.stringify(value);
    }
    new DefinePlugin(definitions).apply();
  }
}

/** `{ $: 'jquery' }`: a module that uses the free variable `$` gets `import $ from 'jquery'`. */
class ProvidePlugin {
  constructor(definitions) {
    this.definitions = definitions;
  }
  apply() {
    wp.provide = { ...(wp.provide ?? {}), ...this.definitions };
  }
}

class BannerPlugin {
  constructor(options) {
    if (typeof options === 'string' || typeof options === 'function') options = { banner: options };
    this.options = options;
    const banner = options.banner;
    this.banner = typeof banner === 'function' ? banner : () => banner;
  }
  apply(compiler) {
    const options = this.options;
    const banner = this.banner;
    compiler.hooks.compilation.tap('BannerPlugin', (compilation) => {
      compilation.hooks.processAssets.tap({ name: 'BannerPlugin', stage: options.stage ?? Compilation.PROCESS_ASSETS_STAGE_ADDITIONS }, () => {
        for (const chunk of compilation.chunks) {
          if (options.entryOnly && !chunk.canBeInitial()) continue;
          for (const file of chunk.files) {
            if (!ModuleFilenameHelpers.matchObject({ test: options.test, include: options.include, exclude: options.exclude }, file)) continue;
            const data = { chunk, filename: file };
            const text = compilation.getPath(banner(data), data);
            const comment = options.raw ? text : wrapComment(text);
            compilation.updateAsset(file, (old) => (options.footer ? new ConcatSource(old, '\n', comment) : new ConcatSource(comment, '\n', old)));
          }
        }
      });
    });
  }
}

class IgnorePlugin {
  constructor(options) {
    this.options = options;
    this.checkIgnore = (resolveData) => {
      const { resourceRegExp, contextRegExp, checkResource } = this.options;
      const request = resolveData.request;
      if (typeof checkResource === 'function') return checkResource(request, resolveData.context) ? true : undefined;
      if (resourceRegExp && ((resourceRegExp.lastIndex = 0), resourceRegExp.test(request))) {
        if (!contextRegExp || ((contextRegExp.lastIndex = 0), contextRegExp.test(resolveData.context))) return true;
      }
      return undefined;
    };
  }
  apply(compiler) {
    compiler.hooks.normalModuleFactory.tap('IgnorePlugin', (nmf) => {
      nmf.hooks.beforeResolve.tap('IgnorePlugin', (resolveData) => (this.checkIgnore(resolveData) ? false : undefined));
    });
  }
}

class NormalModuleReplacementPlugin {
  constructor(resourceRegExp, newResource) {
    this.resourceRegExp = resourceRegExp;
    this.newResource = newResource;
  }
  apply(compiler) {
    const { resourceRegExp, newResource } = this;
    compiler.hooks.normalModuleFactory.tap('NormalModuleReplacementPlugin', (nmf) => {
      nmf.hooks.beforeResolve.tap('NormalModuleReplacementPlugin', (result) => {
        if (((resourceRegExp.lastIndex = 0), resourceRegExp.test(result.request))) {
          if (typeof newResource === 'function') newResource(result);
          else result.request = newResource;
        }
      });
      nmf.hooks.afterResolve.tap('NormalModuleReplacementPlugin', (result) => {
        const createData = result.createData;
        if (((resourceRegExp.lastIndex = 0), resourceRegExp.test(createData.resource))) {
          if (typeof newResource === 'function') newResource(result);
          else {
            if (newResource.startsWith('/') || (newResource.length > 1 && newResource[1] === ':')) createData.resource = newResource;
            else createData.resource = path.join(path.dirname(createData.resource), newResource);
          }
        }
      });
    });
  }
}

/** Plugins that tune webpack's own machinery, which this bundler does not have: they do nothing here. */
const NOOP_PLUGINS = ['LoaderTargetPlugin', 'ContextReplacementPlugin', 'LoaderOptionsPlugin', 'ProgressPlugin', 'HotModuleReplacementPlugin', 'SourceMapDevToolPlugin', 'EvalSourceMapDevToolPlugin', 'EvalDevToolModulePlugin', 'LimitChunkCountPlugin', 'MinChunkSizePlugin', 'ModuleConcatenationPlugin', 'SplitChunksPlugin', 'RuntimeChunkPlugin', 'AggressiveMergingPlugin', 'AggressiveSplittingPlugin', 'NoEmitOnErrorsPlugin', 'NamedModulesPlugin', 'NamedChunksPlugin', 'HashedModuleIdsPlugin', 'DeterministicModuleIdsPlugin', 'DeterministicChunkIdsPlugin', 'OccurrenceOrderPlugin', 'AutomaticPrefetchPlugin', 'PrefetchPlugin', 'WatchIgnorePlugin', 'ExternalsPlugin', 'ProfilingPlugin', 'DllPlugin', 'DllReferencePlugin', 'SideEffectsFlagPlugin', 'FlagDependencyUsagePlugin', 'DefinePluginCompat', 'AutoPublicPathRuntimeModule'];
const noopPlugins = Object.fromEntries(
  NOOP_PLUGINS.map((name) => {
    const cls = { [name]: class { constructor(...args) { this.options = args[0]; } apply() {} } }[name];
    return [name, cls];
  }),
);

class EntryPlugin {
  constructor(context, entry, options) {
    this.context = context;
    this.entry = entry;
    this.options = typeof options === 'string' ? { name: options } : options ?? {};
  }
  apply(compiler) {
    compiler.hooks.make.tapAsync('EntryPlugin', (compilation, callback) => {
      if (compiler._entries) {
        compiler._entries.push({ context: this.context, request: this.entry, name: this.options.name });
        return callback();
      }
      const name = this.options.name ?? path.basename(this.entry, path.extname(this.entry));
      const fileName = `${name}.js`;
      if (!usedChunkNames.has(fileName)) {
        usedChunkNames.add(fileName);
        emittedChunks.push({ reference: `entry:${emittedChunks.length}`, id: this.entry, importer: path.join(this.context ?? root, 'noop.js'), fileName, owner: 'build', resolved: null });
      }
      callback();
    });
  }
  static createDependency(entry, options) {
    return { request: entry, options };
  }
}

// ---- the webpack namespace --------------------------------------------------------------------

const webpackNamespace = {
  version: '5.99.0',
  WebpackError,
  Compiler,
  Compilation,
  Chunk,
  ChunkGroup,
  Entrypoint,
  Stats,
  NormalModule,
  Module: NormalModule,
  sources: webpackSources,
  util: { createHash: hashAdapter, comparators: { compareIds: (a, b) => (a < b ? -1 : a > b ? 1 : 0), compareStringsNumeric: (a, b) => String(a).localeCompare(String(b), undefined, { numeric: true }) }, cleverMerge: (a, b) => ({ ...a, ...b }), LazySet, serialization: { register() {}, registerLoader() {}, registerNotSerializable() {}, createFileSerializer: () => ({}) }, runtime: { forEachRuntime() {}, getEntryRuntime: () => 'main', mergeRuntime: (a) => a }, identifier: { makePathsRelative: (ctx, id) => contextify(ctx, id), contextify, absolutify: (ctx, rq) => path.resolve(ctx, rq) }, fs: {} },
  ModuleFilenameHelpers,
  Template: { getFunctionContent: (fn) => fn.toString(), toIdentifier: (s) => String(s).replace(/[^a-zA-Z0-9_$]/g, '_'), toComment: (s) => `/* ${String(s).replace(/\*\//g, '* /')} */`, asString: (a) => (Array.isArray(a) ? a.join('\n') : a), indent: (s) => (Array.isArray(s) ? s : String(s).split('\n')).map((l) => (l ? '\t' + l : l)).join('\n'), prefix: (s, p) => (Array.isArray(s) ? s : String(s).split('\n')).map((l) => (l ? p + l : l)).join('\n'), numberToIdentifier: (n) => `_${n}`, NUMBER_OF_IDENTIFIER_START_CHARS: 54, NUMBER_OF_IDENTIFIER_CONTINUATION_CHARS: 64 },
  RuntimeGlobals: new Proxy({}, { get: (t, name) => (typeof name === 'string' ? `__webpack_${name}__` : undefined) }),
  RuntimeModule: class RuntimeModule { constructor(name) { this.name = name; } },
  DefinePlugin,
  EnvironmentPlugin,
  ProvidePlugin,
  BannerPlugin,
  IgnorePlugin,
  NormalModuleReplacementPlugin,
  EntryPlugin,
  ...noopPlugins,
  optimize: { LimitChunkCountPlugin: noopPlugins.LimitChunkCountPlugin, MinChunkSizePlugin: noopPlugins.MinChunkSizePlugin, ModuleConcatenationPlugin: noopPlugins.ModuleConcatenationPlugin, SplitChunksPlugin: noopPlugins.SplitChunksPlugin, RuntimeChunkPlugin: noopPlugins.RuntimeChunkPlugin, AggressiveMergingPlugin: noopPlugins.AggressiveMergingPlugin, AggressiveSplittingPlugin: noopPlugins.AggressiveSplittingPlugin },
  node: { NodeTargetPlugin: class { apply() {} }, NodeTemplatePlugin: class { apply() {} }, NodeEnvironmentPlugin: class { apply() {} } },
  web: { FetchCompileWasmPlugin: class { apply() {} }, JsonpTemplatePlugin: class { apply() {} } },
  library: { EnableLibraryPlugin: class { apply() {} }, AbstractLibraryPlugin: class {} },
  javascript: { JavascriptModulesPlugin: { getCompilationHooks: (compilation) => (compilation._jsHooks ??= tolerant({ renderManifest: new SyncWaterfallHook(['result', 'options']), render: new SyncWaterfallHook(['source', 'renderContext']), renderMain: new SyncWaterfallHook(['source']), renderChunk: new SyncWaterfallHook(['source']), chunkHash: new SyncHook(['chunk', 'hash', 'context']), embedInRuntimeBailout: new SyncBailHook(['module']) })) }, EnableChunkLoadingPlugin: class { apply() {} } },
  dependencies: { ModuleDependency: class ModuleDependency { constructor(request) { this.request = request; } }, NullDependency: class NullDependency {}, ConstDependency: class ConstDependency {}, HarmonyImportDependency: class HarmonyImportDependency {} },
  container: {},
  config: { getNormalizedWebpackOptions: (o) => o, applyWebpackOptionsDefaults() {} },
  cache: { MemoryCachePlugin: class { apply() {} } },
  validateSchema: () => {},
};
const webpackFunction = Object.assign(
  () => {
    throw new Error('webpack() is not available: rathkit-pack runs the build; put the plugins in the config');
  },
  webpackNamespace,
);
webpackNamespace.default = webpackFunction;
webpackNamespace.webpack = webpackFunction;

// ---- driving the hooks -------------------------------------------------------------------------

const wp = {
  /** The plugins from the config that are written for webpack's compiler. */
  plugins: [],
  /** The webpack-style parts of the config (output, mode, optimization, ...). */
  config: {},
  compiler: null,
  compilation: null,
  nmf: null,
  runs: 0,
  provide: null,
  active: false,
};

const wpOptions = () => {
  const c = wp.config ?? {};
  const outDir = cli.outdir ?? (cli.outfile ? path.dirname(cli.outfile) : 'dist');
  const mode = c.mode ?? cli.mode ?? 'production';
  return {
    name: c.name,
    mode,
    context: root,
    entry: c.entry ?? c.input ?? cli.entries ?? [],
    devtool: c.devtool ?? (cli.sourcemap ? 'source-map' : false),
    target: c.target ?? 'web',
    externals: c.externals ?? [],
    output: { path: path.resolve(root, outDir), publicPath: 'auto', filename: '[name].js', chunkFilename: '[name].js', assetModuleFilename: '[hash][ext][query]', hashFunction: 'sha256', hashDigest: 'hex', hashDigestLength: 20, hashSalt: undefined, uniqueName: '', clean: false, environment: {}, module: false, globalObject: 'self', charset: true, crossOriginLoading: false, compareBeforeEmit: true, iife: true, pathinfo: false, library: undefined, ...(c.output ?? {}) },
    resolve: c.resolve ?? {},
    resolveLoader: c.resolveLoader ?? {},
    module: c.module ?? { rules: [] },
    plugins: wp.plugins,
    optimization: { minimize: !!cli.minify, minimizer: [], splitChunks: false, runtimeChunk: false, moduleIds: 'deterministic', chunkIds: 'deterministic', nodeEnv: mode, usedExports: true, sideEffects: true, concatenateModules: true, emitOnErrors: true, removeAvailableModules: false, ...(c.optimization ?? {}) },
    performance: c.performance ?? false,
    stats: c.stats ?? 'normal',
    experiments: c.experiments ?? {},
    node: c.node ?? {},
    watch: !!watchMode,
    watchOptions: c.watchOptions ?? {},
    cache: false,
    snapshot: {},
    infrastructureLogging: { level: 'info' },
    ignoreWarnings: c.ignoreWarnings,
    amd: c.amd,
    bail: c.bail,
    profile: false,
    recordsPath: undefined,
    dependencies: [],
    parallelism: 1,
  };
};

const getCompiler = () => {
  if (!wp.compiler) {
    wp.compiler = new Compiler(root, wpOptions());
    wp.compilation = new Compilation(wp.compiler, { normalModuleFactory: makeNormalModuleFactory(), contextModuleFactory: { hooks: tolerant({}) } });
  }
  return wp.compiler;
};
const getCompilation = () => (getCompiler(), wp.compilation);

const makeNormalModuleFactory = () => {
  const hooks = tolerant({
    beforeResolve: new AsyncSeriesBailHook(['resolveData']),
    factorize: new AsyncSeriesBailHook(['resolveData']),
    resolve: new AsyncSeriesBailHook(['resolveData']),
    resolveForScheme: new HookMap(() => new AsyncSeriesBailHook(['resourceData', 'resolveData'])),
    afterResolve: new AsyncSeriesBailHook(['resolveData']),
    createModule: new AsyncSeriesBailHook(['createData', 'resolveData']),
    module: new SyncWaterfallHook(['module', 'createData', 'resolveData']),
    createParser: new HookMap(() => new SyncBailHook(['parserOptions'])),
    parser: new HookMap(() => new SyncHook(['parser', 'parserOptions'])),
    createGenerator: new HookMap(() => new SyncBailHook(['generatorOptions'])),
    generator: new HookMap(() => new SyncHook(['generator', 'generatorOptions'])),
  });
  return { hooks, resolverFactory: wp.compiler?.resolverFactory, getParser: () => ({ hooks: tolerant({}) }), getGenerator: () => ({}) };
};

/** `apply` for every plugin, and the hooks that webpack runs when the compiler is created. */
async function wpInit() {
  const compiler = getCompiler();
  for (const plugin of wp.plugins) {
    if (typeof plugin === 'function') plugin.call(compiler, compiler);
    else plugin.apply(compiler);
  }
  compiler.hooks.environment.call();
  compiler.hooks.afterEnvironment.call();
  compiler.hooks.entryOption.call(root, wp.config.entry ?? cli.entries ?? []);
  compiler.hooks.afterPlugins.call(compiler);
  compiler.hooks.afterResolvers.call(compiler);
  compiler.hooks.initialize.call();
}

/** The start of a build: everything up to `make`, with a fresh compilation. */
async function wpBuildStart() {
  const c = wp.compiler;
  wp.nmf = makeNormalModuleFactory();
  const params = { normalModuleFactory: wp.nmf, contextModuleFactory: { hooks: tolerant({}) } };
  c.watchMode = watchMode;
  c.options.watch = !!watchMode;
  c.running = true;
  await c.hooks.beforeRun.promise(c);
  await (watchMode ? c.hooks.watchRun : c.hooks.run).promise(c);
  c.hooks.normalModuleFactory.call(wp.nmf);
  c.hooks.contextModuleFactory.call(params.contextModuleFactory);
  await c.hooks.beforeCompile.promise(params);
  c.hooks.compile.call(params);
  const compilation = new Compilation(c, params);
  wp.compilation = compilation;
  c.hooks.thisCompilation.call(compilation, params);
  c.hooks.compilation.call(compilation, params);
  await c.hooks.make.promise(compilation);
  await c.hooks.finishMake.promise(compilation);
}

/** The finished module graph becomes the compilation's modules. */
async function wpModules(modules) {
  const compilation = wp.compilation;
  const byId = compilation._modulesById;
  for (const m of modules) {
    const info = moduleInfo.get(m.id) ?? {};
    const module = new NormalModule(m.id, { esm: m.esm, exports: m.exports, importers: m.importers, importedIds: m.importedIds, code: info.code ?? null, sideEffects: m.sideEffects });
    byId.set(m.id, module);
    compilation.modules.add(module);
    compilation.fileDependencies.add(m.id.split('?')[0]);
  }
  for (const module of compilation.modules) compilation.hooks.buildModule.call(module);
  for (const module of compilation.modules) compilation.hooks.succeedModule.call(module);
  await compilation.hooks.finishModules.promise(compilation.modules);
}

/** Seal: chunks and entrypoints from what the bundler made, then the hooks webpack runs through to `processAssets`. */
async function wpSeal(bundle) {
  const compilation = wp.compilation;
  const c = compilation.compiler;
  compilation.hooks.seal.call();
  const chunksByFile = new Map();
  for (const [fileName, item] of Object.entries(bundle)) {
    if (item.type !== 'chunk') continue;
    const chunk = compilation.addChunk(item.name || fileName.replace(/\.[cm]?js$/, ''));
    chunk.id = chunk.name;
    chunk.ids = [chunk.id];
    chunk.files.add(fileName);
    chunk.hash = hashText(item.code, 20);
    chunk.renderedHash = chunk.hash.slice(0, 8);
    chunk.contentHash = { javascript: chunk.hash };
    chunk._initial = !!item.isEntry;
    chunk._entry = !!item.isEntry;
    chunk._modules = (item.moduleIds ?? []).map((id) => compilation._modulesById.get(id)).filter(Boolean);
    chunk._entryModule = item.facadeModuleId ? compilation._modulesById.get(item.facadeModuleId) : undefined;
    chunksByFile.set(fileName, chunk);
    compilation.assets[fileName] = new RawSource(item.code);
    compilation.assetsInfo.set(fileName, { javascriptModule: false, minimized: !!cli.minify });
  }
  for (const [fileName, item] of Object.entries(bundle)) {
    if (item.type === 'chunk') continue;
    if (!compilation.assets[fileName]) compilation.assets[fileName] = new RawSource(item.source);
    if (!compilation.assetsInfo.has(fileName)) compilation.assetsInfo.set(fileName, {});
    // A style sheet belongs to the chunk it is named after.
    const owner = chunksByFile.get(`${fileName.replace(/\.css$/, '')}.js`) ?? chunksByFile.get(`${fileName.replace(/\.css$/, '')}.mjs`);
    if (owner && fileName.endsWith('.css')) owner.files.add(fileName);
  }
  for (const [fileName, item] of Object.entries(bundle)) {
    if (item.type !== 'chunk' || !item.isEntry) continue;
    const chunk = chunksByFile.get(fileName);
    const deps = (item.imports ?? []).map((f) => chunksByFile.get(f)).filter(Boolean);
    const group = new Entrypoint(chunk.name, [...deps, chunk]);
    for (const k of group.chunks) k._groups.add(group);
    compilation.entrypoints.set(chunk.name, group);
    compilation.namedChunkGroups.set(chunk.name, group);
    compilation.chunkGroups.push(group);
  }
  const modules = compilation.modules;
  const chunks = compilation.chunks;
  const h = compilation.hooks;
  h.optimizeDependencies.call(modules);
  h.afterOptimizeDependencies.call(modules);
  h.beforeChunks.call();
  h.afterChunks.call(chunks);
  h.optimize.call();
  h.optimizeModules.call(modules);
  h.afterOptimizeModules.call(modules);
  h.optimizeChunks.call(chunks, compilation.chunkGroups);
  h.afterOptimizeChunks.call(chunks, compilation.chunkGroups);
  await h.optimizeTree.promise(chunks, modules);
  h.afterOptimizeTree.call(chunks, modules);
  await h.optimizeChunkModules.promise(chunks, modules);
  h.afterOptimizeChunkModules.call(chunks, modules);
  h.shouldRecord.call();
  h.reviveModules.call(modules, c.records);
  h.beforeModuleIds.call(modules);
  h.moduleIds.call(modules);
  h.optimizeModuleIds.call(modules);
  h.afterOptimizeModuleIds.call(modules);
  h.reviveChunks.call(chunks, c.records);
  h.beforeChunkIds.call(chunks);
  h.chunkIds.call(chunks);
  h.optimizeChunkIds.call(chunks);
  h.afterOptimizeChunkIds.call(chunks);
  h.recordModules.call(modules, c.records);
  h.recordChunks.call(chunks, c.records);
  h.optimizeCodeGeneration.call(modules);
  h.beforeModuleHash.call();
  h.afterModuleHash.call();
  h.beforeCodeGeneration.call();
  h.afterCodeGeneration.call();
  h.beforeRuntimeRequirements.call();
  h.afterRuntimeRequirements.call();
  h.beforeHash.call();
  compilation.hash = hashText([...chunks].map((k) => k.hash).join(''), 20);
  compilation.fullHash = compilation.hash;
  h.fullHash.call({ digest: () => compilation.hash, update() {} });
  for (const chunk of chunks) h.contentHash.call(chunk);
  h.afterHash.call();
  h.recordHash.call(c.records);
  h.beforeModuleAssets.call();
  h.shouldGenerateChunkAssets.call();
  h.beforeChunkAssets.call();
  for (const chunk of chunks) for (const file of chunk.files) h.chunkAsset.call(chunk, file);
  h.additionalChunkAssets.call(chunks);
  await h.additionalAssets.promise();
  await h.optimizeChunkAssets.promise(chunks);
  h.afterOptimizeChunkAssets.call(chunks);
  await h.optimizeAssets.promise(compilation.assets);
  h.afterOptimizeAssets.call(compilation.assets);
  await h.processAssets.promise(compilation.assets);
  h.afterProcessAssets.call(compilation.assets);
  await h.processAdditionalAssets.promise(compilation.assets);
  await h.afterSeal.promise();
  await c.hooks.afterCompile.promise(compilation);
}

/** What `emit` hands over: whether to write at all, and the assets plugins left. */
async function wpEmit() {
  const compilation = wp.compilation;
  const c = compilation.compiler;
  if (c.hooks.shouldEmit.call(compilation) === false) return false;
  await c.hooks.emit.promise(compilation);
  return true;
}

/** The compilation's assets go back into the bundle the host hands to Rollup-style hooks. */
function wpSync(bundle) {
  const compilation = wp.compilation;
  for (const fileName of Object.keys(bundle)) if (!(fileName in compilation.assets)) delete bundle[fileName];
  for (const [fileName, source] of Object.entries(compilation.assets)) {
    const existing = bundle[fileName];
    if (existing?.type === 'chunk') {
      const text = sourceText(source);
      if (text !== existing.code) {
        // A banner (or any text in front) moves the lines of the source map down.
        const at = text.indexOf(existing.code);
        if (existing.map && at > 0 && text.slice(0, at).endsWith('\n')) existing.map = shiftMapObject(existing.map, text.slice(0, at).split('\n').length - 1);
        else if (existing.map && at !== 0) {
          log('warn', `an asset plugin changed ${fileName}; its source map is dropped`);
          existing.map = null;
        }
        existing.code = text;
      }
    } else if (existing) {
      if (!sameBytes(existing.source, source)) existing.source = Buffer.isBuffer(existing.source) || typeof existing.source !== 'string' ? sourceBytes(source) : sourceText(source);
    } else {
      const value = source.source();
      bundle[fileName] = { type: 'asset', fileName, name: fileName, names: [fileName], originalFileName: null, originalFileNames: [], source: Buffer.isBuffer(value) ? value : value, needsCodeReference: false };
    }
  }
}

const shiftMapObject = (map, lines) => {
  const object = typeof map === 'string' ? JSON.parse(map) : map;
  return { ...object, mappings: ';'.repeat(lines) + object.mappings };
};

async function wpAfterEmit(names, bundle) {
  const compilation = wp.compilation;
  const c = compilation.compiler;
  for (const [fileName, source] of Object.entries(compilation.assets)) {
    compilation.emittedAssets.add(fileName);
    await c.hooks.assetEmitted.promise(fileName, { content: sourceBytes(source), source, outputPath: c.outputPath, targetPath: path.join(c.outputPath, fileName), compilation });
  }
  await c.hooks.afterEmit.promise(compilation);
  compilation.endTime = Date.now();
  const stats = new Stats(compilation);
  for (const warning of compilation.warnings) log('warn', warning?.message ?? String(warning));
  for (const error of compilation.errors) log('error', error?.message ?? String(error));
  await c.hooks.done.promise(stats);
  c.running = false;
}

/** The pieces of the build that run as one plugin among the others. */
const wpAdapter = () => ({
  name: 'webpack-compiler',
  async buildStart() {
    await wpBuildStart();
  },
  async resolveId(source, importer, options) {
    const nmf = wp.nmf;
    if (!nmf || (!nmf.hooks.beforeResolve.isUsed() && !nmf.hooks.afterResolve.isUsed())) return null;
    const data = { contextInfo: { issuer: importer ?? '', issuerLayer: undefined, compiler: undefined }, resolveOptions: {}, context: importer ? path.dirname(importer) : root, request: source, assertions: undefined, dependencies: [{ request: source, category: 'esm', type: 'import' }], dependencyType: options.kind === 'require-call' ? 'commonjs' : 'esm', fileDependencies: new LazySet(), missingDependencies: new LazySet(), contextDependencies: new LazySet(), createData: {}, cacheable: true };
    if ((await nmf.hooks.beforeResolve.promise(data)) === false) return { id: `\0rathkit:ignored:${source}`, moduleSideEffects: false };
    const resolved = await this.resolve(data.request, importer, { skipSelf: true, kind: options.kind, isEntry: options.isEntry });
    if (!resolved) return data.request !== source ? { id: data.request } : null;
    if (nmf.hooks.afterResolve.isUsed()) {
      data.createData = { resource: resolved.id, request: resolved.id, userRequest: source, rawRequest: source, context: data.context, loaders: [], matchResource: undefined, settings: {}, type: 'javascript/auto', resolveOptions: {} };
      if ((await nmf.hooks.afterResolve.promise(data)) === false) return { id: `\0rathkit:ignored:${source}`, moduleSideEffects: false };
      if (data.createData.resource !== resolved.id) return { id: data.createData.resource };
    }
    return resolved.external ? { id: resolved.id, external: true } : { id: resolved.id };
  },
  // A module a plugin ignored is an empty one.
  load(id) {
    return id.startsWith('\0rathkit:ignored:') ? { code: 'module.exports = {};', moduleType: 'js' } : null;
  },
  async buildEnd(error) {
    if (error) wp.compiler.hooks.failed.call(error);
  },
  async generateBundle(options, bundle) {
    await wpSeal(bundle);
    const emit = await wpEmit();
    if (!emit) for (const name of Object.keys(bundle)) delete bundle[name];
    else wpSync(bundle);
  },
  async writeBundle(options, bundle) {
    await wpAfterEmit(Object.keys(bundle), bundle);
  },
  async closeBundle() {
    if (!watchMode) await wp.compiler.hooks.shutdown.promise();
  },
  async closeWatcher() {
    wp.compiler.hooks.watchClose.call();
    await wp.compiler.hooks.shutdown.promise();
  },
  async watchChange(id, { event }) {
    wp.compiler.hooks.invalid.call(id, Date.now());
    wp.compiler.modifiedFiles = new Set(event === 'delete' ? [] : [id]);
    wp.compiler.removedFiles = new Set(event === 'delete' ? [id] : []);
  },
});

/** ProvidePlugin: `import $ from 'jquery'` in front of modules that use `$` without declaring it. */
const provideAdapter = () => ({
  name: 'webpack-provide',
  transform(code, id) {
    const provide = wp.provide;
    if (!provide || id.startsWith('\0') || !/\.([cm]?[jt]sx?)$/.test(id.split('?')[0])) return null;
    const keys = Object.keys(provide).filter((k) => !k.includes('.') && code.includes(k));
    if (!keys.length) return null;
    let free;
    try {
      free = new Set(JSON.parse(askParser('g', code)));
    } catch {
      return null;
    }
    const lines = [];
    for (const key of keys) {
      if (!free.has(key)) continue;
      const target = provide[key];
      const [module, ...member] = Array.isArray(target) ? target : [target];
      const local = member.length ? `__provided_${key.replace(/\W/g, '_')}` : key;
      lines.push(member.length ? `import { ${member[0]} as ${local} } from ${JSON.stringify(module)};` : `import ${local} from ${JSON.stringify(module)};`);
      if (member.length > 1) lines.push(`const ${key} = ${local}${member.slice(1).map((m) => `[${JSON.stringify(m)}]`).join('')};`);
      else if (member.length) lines.push(`const ${key} = ${local};`);
    }
    // On one line, so lines keep their numbers.
    return lines.length ? { code: lines.join(' ') + ' ' + code, map: null } : null;
  },
});

/** The plugin from the config as a webpack plugin of this file, if it is one of the built-in kinds. */
const builtInPlugin = (raw) => {
  const name = raw?.constructor?.name;
  switch (name) {
    case 'DefinePlugin': return new DefinePlugin(raw.definitions);
    case 'EnvironmentPlugin': return Object.assign(new EnvironmentPlugin([]), { keys: raw.keys, defaultValues: raw.defaultValues });
    case 'ProvidePlugin': return new ProvidePlugin(raw.definitions);
    case 'BannerPlugin': return new BannerPlugin(raw.options);
    case 'IgnorePlugin': return new IgnorePlugin(raw.options);
    case 'NormalModuleReplacementPlugin': return new NormalModuleReplacementPlugin(raw.resourceRegExp, raw.newResource);
    // Something that looks like DefinePlugin (a `definitions` table) is treated as one.
    default: return raw?.definitions && typeof raw.apply === 'function' ? new DefinePlugin(raw.definitions) : null;
  }
};

/** `webpack`, `@parcel/plugin` and friends resolve to this file's versions when the project does not install them. */
const installWebpackShims = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rathkit-shims-'));
  const table = {
    webpack: { value: webpackNamespace, match: (s) => s === 'webpack' || s.startsWith('webpack/') },
    tapable: { value: tapable, match: (s) => s === 'tapable' },
    'webpack-sources': { value: webpackSources, match: (s) => s === 'webpack-sources' },
    '@parcel/plugin': { value: parcelPluginShim, match: (s) => s === '@parcel/plugin' },
    '@parcel/utils': { value: parcelUtilsShim, match: (s) => s === '@parcel/utils' },
    '@parcel/diagnostic': { value: parcelDiagnosticShim, match: (s) => s === '@parcel/diagnostic' },
  };
  globalThis.__rathkitShims = Object.fromEntries(Object.entries(table).map(([k, v]) => [k, v.value]));
  const urls = {};
  const files = {};
  Object.entries(table).forEach(([key, { value }], index) => {
    const names = Object.keys(value).filter((n) => /^[A-Za-z_$][\w$]*$/.test(n) && n !== 'default');
    const file = path.join(dir, `shim${index}.mjs`);
    fs.writeFileSync(file, `const shim = globalThis.__rathkitShims[${JSON.stringify(key)}];\nexport default shim;\nexport { shim as 'module.exports' };\n${names.map((n) => `export const ${n} = shim[${JSON.stringify(n)}];`).join('\n')}\n`);
    urls[key] = pathToFileURL(file).href;
    // `require` gets a CommonJS file, so it does not depend on being able to require an ES module.
    files[key] = path.join(dir, `shim${index}.cjs`);
    fs.writeFileSync(files[key], `module.exports = globalThis.__rathkitShims[${JSON.stringify(key)}];\n`);
  });
  const keyOf = (specifier) => Object.keys(table).find((k) => table[k].match(specifier));
  const missing = (error) => ['ERR_MODULE_NOT_FOUND', 'MODULE_NOT_FOUND', 'ERR_PACKAGE_PATH_NOT_EXPORTED'].includes(error?.code);
  // `require`: every Node version.
  const Module = nodeModule.Module ?? nodeModule.default;
  if (Module && typeof Module._resolveFilename === 'function') {
    const original = Module._resolveFilename;
    Module._resolveFilename = function (request, ...rest) {
      try {
        return original.call(this, request, ...rest);
      } catch (error) {
        const key = typeof request === 'string' ? keyOf(request) : null;
        if (key && missing(error)) return files[key];
        throw error;
      }
    };
  }
  // `import`: through `registerHooks` (Node 22.15, 23.5), or the older asynchronous hooks (Node 18.19, 20.6).
  if (typeof nodeModule.registerHooks === 'function') {
    nodeModule.registerHooks({
      resolve(specifier, context, nextResolve) {
        try {
          return nextResolve(specifier, context);
        } catch (error) {
          const key = keyOf(specifier);
          if (key && missing(error)) return { url: urls[key], shortCircuit: true, format: 'module' };
          throw error;
        }
      },
    });
  } else if (typeof nodeModule.register === 'function') {
    const rules = Object.keys(table).map((k) => [k, k === 'webpack' ? 'prefix' : 'exact']);
    const source = `
const rules = ${JSON.stringify(rules)};
let urls;
export async function initialize(data) { urls = data.urls; }
export async function resolve(specifier, context, nextResolve) {
  try { return await nextResolve(specifier, context); }
  catch (error) {
    const hit = rules.find(([name, mode]) => specifier === name || (mode === 'prefix' && specifier.startsWith(name + '/')));
    if (hit && ['ERR_MODULE_NOT_FOUND', 'MODULE_NOT_FOUND', 'ERR_PACKAGE_PATH_NOT_EXPORTED'].includes(error?.code)) return { url: urls[hit[0]], shortCircuit: true, format: 'module' };
    throw error;
  }
}`;
    nodeModule.register('data:text/javascript;base64,' + Buffer.from(source).toString('base64'), { parentURL: import.meta.url, data: { urls } });
  } else {
    log('warn', `this Node (${process.version}) cannot supply stand-ins for "webpack" or "@parcel/plugin" to \`import\`; a plugin that imports them needs the project to install them`);
  }
  process.on('exit', () => fs.rmSync(dir, { recursive: true, force: true }));
};
