// Parcel plugins for the plugin host: transformers, resolvers, reporters, optimizers, validators,
// compressors, namers and runtimes, as listed in a `.parcelrc`.
//
// This file is appended to host.mjs and shares its scope. Every kind of plugin is turned into a plugin of
// the shape the host already runs (Rollup's hooks):
//
//   Transformer   transform         Resolver     resolveId
//   Validator     moduleParsed      Reporter     buildStart / buildEnd / writeBundle
//   Optimizer     generateBundle    Compressor   generateBundle (a compressed copy next to the file)
//   Namer         generateBundle    Runtime      generateBundle (code added to bundles)
//
// Parcel's own plugins for JavaScript, CSS, HTML, JSON and raw files, its packagers and its default bundler, namer
// and resolver are what this bundler already does, so those entries in a configuration are left out.

const PARCEL_PLUGIN = Symbol.for('parcel-plugin-config');

/** Plugins that are Parcel's own machinery; the bundler does their work itself. */
const PARCEL_CORE = [
  /^@parcel\/transformer-(js|css|html|json|raw|react-refresh-wrap|worklet|webmanifest)$/,
  /^@parcel\/optimizer-(swc|css|htmlnano)$/,
  /^@parcel\/packager-/,
  /^@parcel\/bundler-/,
  /^@parcel\/namer-default$/,
  /^@parcel\/resolver-default$/,
  /^@parcel\/runtime-(js|browser-hmr|react-refresh|service-worker|rsc)$/,
  /^@parcel\/compressor-raw$/,
  /^@parcel\/reporter-(cli|dev-server|tracer|bundle-analyzer|bundle-buddy)$/,
  /^@parcel\/config-/,
];

const parcel = { active: false, config: null, transformers: [], resolvers: [], reporters: [], optimizers: [], validators: [], compressors: [], namers: [], runtimes: [], loaded: new Map() };

// ---- configuration -----------------------------------------------------------------------------

/** `.parcelrc` may have comments and trailing commas. */
const parseLenientJson = (text) => {
  let out = '';
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      out += text.slice(i, j + 1);
      i = j + 1;
    } else if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') i++;
    } else if (c === '/' && text[i + 1] === '*') {
      i = text.indexOf('*/', i + 2) + 2 || text.length;
    } else {
      out += c;
      i++;
    }
  }
  return JSON.parse(out.replace(/,(\s*[}\]])/g, '$1'));
};

const PIPELINE_KINDS = ['transformers', 'optimizers', 'compressors', 'validators', 'packagers'];
const LIST_KINDS = ['resolvers', 'reporters', 'namers', 'runtimes', 'bundler'];

/** Reads a `.parcelrc` and what it extends. Entries named `...` stand for the parent's. */
const readParcelConfig = (file, seen = new Set()) => {
  if (seen.has(file)) return {};
  seen.add(file);
  return parcelConfigFrom(parseLenientJson(fs.readFileSync(file, 'utf8')), path.dirname(file), seen);
};

/** A configuration object (from a file, or written in the rathkit config) merged with what it extends. */
const parcelConfigFrom = (own, dir, seen = new Set()) => {
  const parents = [];
  for (const spec of toArray(own.extends)) {
    if (/^@parcel\/config-/.test(spec)) continue;
    const parentFile =
      spec.startsWith('.') || path.isAbsolute(spec)
        ? path.resolve(dir, spec)
        : (() => {
            const require = createRequire(path.join(dir, 'noop.js'));
            try {
              return require.resolve(spec);
            } catch {
              return require.resolve(`${spec}/package.json`).replace(/package\.json$/, 'index.json');
            }
          })();
    parents.push(readParcelConfig(parentFile, seen));
  }
  const merged = {};
  for (const kind of PIPELINE_KINDS) {
    const map = {};
    for (const parent of parents) for (const [glob, list] of Object.entries(parent[kind] ?? {})) map[glob] = list;
    for (const [glob, list] of Object.entries(own[kind] ?? {})) {
      const names = toArray(list);
      // `...` is the parent's pipeline for the same glob.
      map[glob] = names.flatMap((n) => (n === '...' ? toArray(parents.map((p) => p[kind]?.[glob]).find(Boolean) ?? []) : [n]));
    }
    merged[kind] = map;
  }
  for (const kind of LIST_KINDS) {
    const parentList = parents.flatMap((p) => toArray(p[kind] ?? []));
    merged[kind] = own[kind] === undefined ? parentList : toArray(own[kind]).flatMap((n) => (n === '...' ? parentList : [n]));
  }
  merged._dir = dir;
  return merged;
};

/** The globs of Parcel: `*.svg`, `*.{js,jsx}`, `*.module.css`, matched against the path from the project root, or the file name. */
const parcelGlob = (glob) => {
  let source = '';
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*') {
      if (glob[i + 1] === '*') {
        source += '.*';
        i++;
        if (glob[i + 1] === '/') i++;
      } else source += '[^/]*';
    } else if (c === '?') source += '[^/]';
    else if (c === '{') {
      const end = glob.indexOf('}', i);
      source += '(?:' + glob.slice(i + 1, end).split(',').map((s) => parcelGlob(s).source.slice(1, -1)).join('|') + ')';
      i = end;
    } else source += /[.+^$()|\\[\]]/.test(c) ? '\\' + c : c;
  }
  return new RegExp(`^${source}$`);
};
const globMatches = (glob, file) => {
  const re = parcelGlob(glob);
  const relative = path.relative(root, file).split(path.sep).join('/');
  return re.test(relative) || re.test(path.basename(file)) || (!glob.includes('/') && re.test(relative.split('/').pop()));
};

const loadParcelPlugin = async (name, kind) => {
  if (PARCEL_CORE.some((re) => re.test(name))) return null;
  if (parcel.loaded.has(`${kind}:${name}`)) return parcel.loaded.get(`${kind}:${name}`);
  let plugin = null;
  try {
    const require = createRequire(path.join(root, 'noop.js'));
    const file = name.startsWith('.') || path.isAbsolute(name) ? path.resolve(root, name) : require.resolve(name);
    const mod = await import(pathToFileURL(file).href);
    const instance = mod.default?.default ?? mod.default ?? mod;
    const implementation = instance?.[PARCEL_PLUGIN] ?? instance;
    plugin = { name, ...implementation, implementation };
    plugin.implementation = implementation;
    plugin.pluginName = name;
  } catch (error) {
    log('warn', `the Parcel plugin ${name} could not be loaded (${error.message}); it was left out`);
  }
  parcel.loaded.set(`${kind}:${name}`, plugin);
  return plugin;
};

// ---- what plugins are given --------------------------------------------------------------------

const parcelLogger = (name) => ({
  verbose() {},
  info() {},
  log() {},
  warn: (d) => log('warn', `[${name}] ${diagnosticText(d)}`),
  error: (d) => log('error', `[${name}] ${diagnosticText(d)}`),
});
const diagnosticText = (d) => (Array.isArray(d) ? d.map(diagnosticText).join('; ') : typeof d === 'string' ? d : d?.message ?? d?.diagnostic?.message ?? JSON.stringify(d));

const parcelEnv = () => ({
  context: 'browser',
  engines: {},
  includeNodeModules: true,
  outputFormat: cli.format === 'cjs' ? 'commonjs' : 'esmodule',
  sourceType: 'module',
  isLibrary: false,
  shouldOptimize: !!cli.minify,
  shouldScopeHoist: true,
  sourceMap: cli.sourcemap ? {} : undefined,
  loc: undefined,
  isBrowser: () => true,
  isNode: () => false,
  isWorker: () => false,
  isWorklet: () => false,
  isElectron: () => false,
  isIsolated: () => false,
  isESM: () => cli.format !== 'cjs',
  matchesEngines: () => true,
  supports: () => true,
});

const parcelOptions = () => ({
  projectRoot: root,
  cacheDir: path.join(root, 'node_modules/.rathkit'),
  entryRoot: root,
  env: process.env,
  mode: cli.mode ?? 'production',
  hmrOptions: null,
  shouldAutoInstall: false,
  shouldContentHash: true,
  shouldBuildLazily: false,
  logLevel: 'info',
  inputFS: inputFileSystem,
  outputFS: outputFileSystem,
  packageManager: {},
  instanceId: 'rathkit',
  detailedReport: false,
  defaultTargetOptions: { shouldOptimize: !!cli.minify, shouldScopeHoist: true, sourceMaps: !!cli.sourcemap, publicUrl: '/', distDir: path.resolve(root, cli.outdir ?? 'dist'), engines: {} },
});

const toSourceMapObject = (map) => {
  if (!map) return null;
  if (typeof map === 'string') return JSON.parse(map);
  if (Buffer.isBuffer(map)) return JSON.parse(map.toString('utf8'));
  if (typeof map.toVLQ === 'function') {
    const vlq = map.toVLQ();
    return { version: 3, ...vlq };
  }
  if (typeof map.toString === 'function' && map.constructor?.name === 'SourceMap') return JSON.parse(map.toString());
  return map;
};

class ParcelAsset {
  constructor({ id, filePath, code, type, isSource = true, query = '', pipeline, map = null, symbols, meta }) {
    this.id = id;
    this.filePath = filePath;
    this.query = new URLSearchParams(query.replace(/^\?/, ''));
    this.type = type;
    this.pipeline = pipeline;
    this.env = parcelEnv();
    this.isSource = isSource;
    this.meta = meta ?? {};
    this.sideEffects = true;
    this.uniqueKey = undefined;
    this.isBundleSplittable = true;
    this.bundleBehavior = undefined;
    this.symbols = symbols ?? new Map();
    this.stats = { size: Buffer.byteLength(code), time: 0 };
    this._code = code;
    this._map = map;
    this._ast = null;
    this._dependencies = [];
    this._urlDependencies = [];
    this.invalidations = { files: [], env: [], create: [] };
  }
  async getCode() {
    return typeof this._code === 'string' ? this._code : Buffer.from(this._code).toString('utf8');
  }
  async getBuffer() {
    return Buffer.isBuffer(this._code) ? this._code : Buffer.from(this._code);
  }
  getStream() {
    return Readable.from([this._code]);
  }
  async getMap() {
    return this._map ?? null;
  }
  async getMapBuffer() {
    return this._map ? Buffer.from(JSON.stringify(toSourceMapObject(this._map))) : null;
  }
  async getAST() {
    return this._ast;
  }
  setCode(code) {
    this._code = code;
  }
  setBuffer(buffer) {
    this._code = buffer;
  }
  setStream() {
    throw new Error('asset.setStream() is not supported');
  }
  setMap(map) {
    this._map = map;
  }
  setAST(ast) {
    this._ast = ast;
  }
  setEnvironment() {}
  setBundleBehavior(b) {
    this.bundleBehavior = b;
  }
  addDependency(dep) {
    this._dependencies.push(dep);
    return dep.specifier;
  }
  addURLDependency(url, opts = {}) {
    this._urlDependencies.push({ specifier: url, ...opts });
    return url;
  }
  invalidateOnFileChange(f) {
    this.invalidations.files.push(f);
    watchFiles.add(f);
  }
  invalidateOnFileCreate() {}
  invalidateOnEnvChange() {}
  invalidateOnStartup() {}
  invalidateOnBuild() {}
  isASTDirty() {
    return false;
  }
  addIncludedFile(file) {
    this.invalidateOnFileChange(typeof file === 'string' ? file : file.filePath);
  }
  getDependencies() {
    return this._dependencies;
  }
  getIncludedFiles() {
    return [];
  }
}

/** `config` for `loadConfig`: finding files near the asset the way Parcel plugins do. */
const parcelConfigFor = (filePath, plugin) => {
  const searchPath = filePath;
  const result = { value: undefined };
  const read = (file, { packageKey, parse } = {}) => {
    const text = fs.readFileSync(file, 'utf8');
    const ext = path.extname(file);
    let value;
    if (parse === false) value = text;
    else if (ext === '.js' || ext === '.cjs') value = createRequire(file)(file);
    else if (ext === '.mjs') value = null;
    else value = parseLenientJson(text);
    if (packageKey && value) value = value[packageKey];
    return value;
  };
  const find = async (from, filePaths, options) => {
    for (let dir = path.dirname(from); ; dir = path.dirname(dir)) {
      for (const name of filePaths) {
        const candidate = path.join(dir, name);
        if (fs.existsSync(candidate)) {
          const value = read(candidate, name === 'package.json' ? { packageKey: options?.packageKey, ...options } : options);
          if (value !== undefined) return { contents: value, filePath: candidate };
        }
      }
      if (dir === path.dirname(dir) || dir === path.dirname(root)) return null;
    }
  };
  return {
    searchPath,
    isSource: true,
    env: parcelEnv(),
    result: undefined,
    getConfig: (filePaths, options) => find(searchPath, filePaths, options),
    getConfigFrom: (from, filePaths, options) => find(from, filePaths, options),
    getPackage: async () => (await find(searchPath, ['package.json']))?.contents ?? null,
    setResult(value) {
      result.value = value;
      this.result = value;
    },
    invalidateOnFileChange: (f) => watchFiles.add(f),
    invalidateOnFileCreate() {},
    invalidateOnEnvChange() {},
    invalidateOnStartup() {},
    invalidateOnBuild() {},
    addDevDependency() {},
    shouldInvalidateOnStartup() {},
    shouldReload() {},
  };
};

// ---- the adapter -------------------------------------------------------------------------------

const matchingPlugins = (map, file, kind) => {
  const list = [];
  for (const [glob, names] of Object.entries(map)) if (globMatches(glob, file)) list.push(...names);
  return list.filter((n) => typeof n === 'string');
};

const assetTypeToModuleType = (type) => ({ js: 'js', mjs: 'js', cjs: 'js', jsx: 'jsx', ts: 'ts', tsx: 'tsx', json: 'json', css: 'css' })[type];

const loadPipelines = async () => {
  const config = parcel.config;
  const resolved = {};
  for (const kind of ['transformers', 'optimizers', 'compressors', 'validators']) {
    resolved[kind] = {};
    for (const [glob, names] of Object.entries(config[kind] ?? {})) {
      const plugins = [];
      for (const name of toArray(names)) {
        const plugin = await loadParcelPlugin(name, kind);
        if (plugin) plugins.push(plugin);
      }
      if (plugins.length) resolved[kind][glob] = plugins;
    }
  }
  for (const kind of ['resolvers', 'reporters', 'namers', 'runtimes']) {
    resolved[kind] = [];
    for (const name of toArray(config[kind] ?? [])) {
      const plugin = await loadParcelPlugin(name, kind);
      if (plugin) resolved[kind].push(plugin);
    }
  }
  return resolved;
};

const pluginsFor = (map, file) => {
  const out = [];
  for (const [glob, plugins] of Object.entries(map)) if (globMatches(glob, file)) out.push(...plugins);
  return out;
};

const bundleShim = (file, item, chunkInfo) => ({
  id: file,
  type: path.extname(file).slice(1),
  name: file,
  displayName: file,
  filePath: path.join(path.resolve(root, cli.outdir ?? 'dist'), file),
  env: parcelEnv(),
  target: { distDir: path.resolve(root, cli.outdir ?? 'dist'), distEntry: file, publicUrl: '/', env: parcelEnv(), name: 'default' },
  isInline: false,
  isSplittable: true,
  needsStableName: !!chunkInfo?.isEntry,
  bundleBehavior: undefined,
  stats: { size: item.type === 'chunk' ? Buffer.byteLength(item.code) : sourceBytes(new RawSource(item.source)).length, time: 0 },
  hashReference: 'HASH_REF',
  getEntryAssets: () => (chunkInfo?.facadeModuleId ? [{ filePath: chunkInfo.facadeModuleId, id: chunkInfo.facadeModuleId, type: 'js' }] : []),
  getMainEntry: () => (chunkInfo?.facadeModuleId ? { filePath: chunkInfo.facadeModuleId, id: chunkInfo.facadeModuleId, type: 'js' } : null),
  traverseAssets() {},
  traverse() {},
  getContents: () => (item.type === 'chunk' ? item.code : item.source),
});

const bundleGraphShim = (bundle) => {
  const bundles = Object.entries(bundle).map(([file, item]) => bundleShim(file, item, item));
  return {
    getBundles: () => bundles,
    getEntryBundles: () => bundles.filter((b) => bundle[b.name]?.isEntry),
    getChildBundles: () => [],
    getParentBundles: () => [],
    getBundleGroupsContainingBundle: () => [],
    getReferencedBundles: () => [],
    getBundlesInBundleGroup: () => bundles,
    getSiblingBundles: () => [],
    getImportedSymbols: () => [],
    getIncomingDependencies: () => [],
    getDependencies: () => [],
    getUsedSymbols: () => new Set(),
    isAssetReachableFromBundle: () => true,
    traverse() {},
    traverseBundles() {},
    getBundleGroupsReferencedByBundle: () => [],
    getBundleGroupsReferencedByBundle_: () => [],
    getAssetPublicId: (asset) => asset.id,
    getAssetById: () => null,
  };
};

const parcelAdapter = (pipelines) => {
  const reportEvent = async (event) => {
    for (const plugin of pipelines.reporters) {
      if (typeof plugin.report !== 'function') continue;
      try {
        await plugin.report({ event, options: parcelOptions(), logger: parcelLogger(plugin.name) });
      } catch (error) {
        log('warn', `[${plugin.name}] ${error.message}`);
      }
    }
  };
  const virtualCode = new Map(); // id -> { code, dir, type }
  let startedAt = Date.now();
  return {
    name: 'parcel',
    async buildStart() {
      startedAt = Date.now();
      await reportEvent({ type: 'buildStart' });
    },
    async resolveId(source, importer, options) {
      if (virtualCode.has(source)) return { id: source };
      if (!pipelines.resolvers.length || source.startsWith('\0')) return null;
      const dependency = { specifier: source, specifierType: options.kind === 'require-call' ? 'commonjs' : 'esm', sourcePath: importer, resolveFrom: importer, isEntry: !!options.isEntry, isOptional: false, needsStableName: false, priority: options.kind === 'dynamic-import' ? 'lazy' : 'sync', pipeline: undefined, meta: {}, symbols: new Map(), env: parcelEnv(), loc: undefined, range: undefined, packageConditions: undefined, bundleBehavior: undefined };
      for (const plugin of pipelines.resolvers) {
        if (typeof plugin.resolve !== 'function') continue;
        const result = await plugin.resolve({ specifier: source, dependency, options: parcelOptions(), logger: parcelLogger(plugin.name), config: undefined, pipeline: undefined });
        if (!result) continue;
        if (result.isExcluded) return { id: source, external: true };
        if (result.filePath) {
          if (result.code !== undefined) {
            virtualCode.set(result.filePath, { code: result.code, dir: path.dirname(result.filePath) });
          }
          for (const f of result.invalidateOnFileChange ?? []) watchFiles.add(f);
          return { id: result.filePath + (result.query ? '?' + result.query : ''), moduleSideEffects: result.sideEffects, meta: result.meta };
        }
      }
      return null;
    },
    load(id) {
      const entry = virtualCode.get(id);
      return entry ? { code: entry.code, resolveDir: entry.dir } : null;
    },
    async transform(code, id) {
      const file = id.split('?')[0];
      if (id.startsWith('\0') || !path.isAbsolute(file)) return null;
      const plugins = pluginsFor(pipelines.transformers, file);
      if (!plugins.length) return null;
      const type = path.extname(file).slice(1);
      let asset = new ParcelAsset({ id: crypto.createHash('md5').update(file).digest('hex').slice(0, 16), filePath: file, code, type, query: id.includes('?') ? id.slice(id.indexOf('?')) : '' });
      let changed = false;
      const extras = [];
      for (const plugin of plugins) {
        if (typeof plugin.transform !== 'function' && typeof plugin.parse !== 'function') continue;
        const config = parcelConfigFor(file, plugin);
        const logger = parcelLogger(plugin.name);
        const options = parcelOptions();
        let configResult;
        if (typeof plugin.loadConfig === 'function') {
          configResult = await plugin.loadConfig({ config, options, logger });
          configResult = config.result ?? configResult;
        }
        if (typeof plugin.canReuseAST === 'function' || typeof plugin.parse === 'function') {
          if (typeof plugin.parse === 'function') asset.setAST(await plugin.parse({ asset, config: configResult, resolve: async (from, specifier) => (await resolveId(specifier, from, { kind: 'import-statement' }, null))?.id, options, logger }));
        }
        let results = [asset];
        if (typeof plugin.transform === 'function') {
          results = await plugin.transform({ asset, config: configResult, resolve: async (from, specifier) => (await resolveId(specifier, from, { kind: 'import-statement' }, null))?.id, options, logger });
          if (!Array.isArray(results)) results = [results ?? asset];
        }
        if (typeof plugin.generate === 'function' && results[0]?._ast) {
          const generated = await plugin.generate({ asset: results[0], ast: results[0]._ast, options, logger });
          results[0].setCode(generated.content);
          if (generated.map) results[0].setMap(generated.map);
        }
        if (typeof plugin.postProcess === 'function') results = (await plugin.postProcess({ assets: results, config: configResult, options, logger })) ?? results;
        asset = results[0];
        extras.push(...results.slice(1));
        changed = true;
      }
      if (!changed) return null;
      let output = await asset.getCode();
      // Dependencies a transformer added that its code does not mention itself.
      const added = [...asset._dependencies, ...asset._urlDependencies].map((d) => d.specifier).filter((s) => !output.includes(JSON.stringify(s)) && !output.includes(`'${s}'`));
      const type_ = asset.type;
      if (added.length && (type_ === 'js' || type_ === 'jsx' || type_ === 'ts' || type_ === 'tsx')) output = added.map((s) => `import ${JSON.stringify(s)};`).join('') + output;
      // Other assets the transformer made are modules of their own, reached by their unique key.
      for (const extra of extras) {
        const key = extra.uniqueKey ?? extra.id;
        virtualCode.set(key, { code: await extra.getCode(), dir: path.dirname(file) });
      }
      const map = toSourceMapObject(await asset.getMap());
      return { code: output, map: map ? JSON.stringify(map) : undefined, moduleType: assetTypeToModuleType(asset.type) ?? (asset.type !== type ? asset.type : undefined) };
    },
    async moduleParsed(info) {
      for (const [glob, validators] of Object.entries(pipelines.validators)) {
        const file = info.id.split('?')[0];
        if (!globMatches(glob, file)) continue;
        // A file nobody transformed was read by the bundler; read it here as well.
        const code = info.code ?? (path.isAbsolute(file) && fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null);
        if (code === null) continue;
        for (const validator of validators) {
          if (typeof validator.validate !== 'function') continue;
          const asset = new ParcelAsset({ id: info.id, filePath: file, code, type: path.extname(file).slice(1) });
          const result = await validator.validate({ asset, config: undefined, options: parcelOptions(), logger: parcelLogger(validator.name) });
          for (const w of result?.warnings ?? []) log('warn', `[${validator.name}] ${diagnosticText(w)}`);
          for (const e of result?.errors ?? []) log('error', `[${validator.name}] ${diagnosticText(e)}`);
        }
      }
    },
    async generateBundle(options, bundle) {
      const graph = bundleGraphShim(bundle);
      const outDir = path.resolve(root, cli.outdir ?? 'dist');
      // Runtimes add code to bundles.
      for (const runtime of pipelines.runtimes) {
        if (typeof runtime.apply !== 'function') continue;
        for (const shim of graph.getBundles()) {
          const item = bundle[shim.name];
          if (item.type !== 'chunk') continue;
          const added = (await runtime.apply({ bundle: shim, bundleGraph: graph, options: parcelOptions(), config: undefined, logger: parcelLogger(runtime.name) })) ?? [];
          for (const a of toArray(added)) if (a?.code) item.code = a.isEntry ? `${a.code}\n${item.code}` : `${item.code}\n${a.code}`;
        }
      }
      // Optimizers: the contents of each file they match.
      for (const [glob, optimizers] of Object.entries(pipelines.optimizers)) {
        for (const shim of graph.getBundles()) {
          if (!globMatches(glob, shim.name)) continue;
          const item = bundle[shim.name];
          for (const optimizer of optimizers) {
            if (typeof optimizer.optimize !== 'function') continue;
            const contents = item.type === 'chunk' ? item.code : item.source;
            const result = await optimizer.optimize({ bundle: shim, bundleGraph: graph, contents, map: null, getSourceMapReference: (m) => (m ? `//# sourceMappingURL=${shim.name}.map\n` : null), options: parcelOptions(), config: undefined, logger: parcelLogger(optimizer.name) });
            if (result?.contents !== undefined) {
              const text = await blobText(result.contents);
              if (item.type === 'chunk') item.code = text;
              else item.source = Buffer.isBuffer(item.source) ? Buffer.from(text) : text;
              if (result.map && item.type === 'chunk') item.map = toSourceMapObject(result.map);
            }
          }
        }
      }
      // Namers: new names for files; other files that mention the old name follow.
      if (pipelines.namers.length) {
        const renames = new Map();
        for (const shim of graph.getBundles()) {
          for (const namer of pipelines.namers) {
            if (typeof namer.name !== 'function') continue;
            const named = await namer.name({ bundle: shim, bundleGraph: graph, options: parcelOptions(), config: undefined, logger: parcelLogger(namer.name) });
            if (named && named !== shim.name) {
              renames.set(shim.name, path.basename(named) === named ? named : named);
              break;
            }
          }
        }
        for (const [from, to] of renames) {
          if (bundle[to]) continue;
          bundle[to] = { ...bundle[from], fileName: to };
          delete bundle[from];
        }
        for (const item of Object.values(bundle)) {
          if (item.type !== 'chunk') continue;
          for (const [from, to] of renames) item.code = item.code.split(`./${from}`).join(`./${to}`);
        }
      }
      // Compressors: a compressed copy next to each file they match.
      for (const [glob, compressors] of Object.entries(pipelines.compressors)) {
        for (const shim of graph.getBundles()) {
          if (!globMatches(glob, shim.name)) continue;
          const item = bundle[shim.name];
          if (!item) continue;
          for (const compressor of compressors) {
            if (typeof compressor.compress !== 'function') continue;
            const data = item.type === 'chunk' ? Buffer.from(item.code) : sourceBytes(new RawSource(item.source));
            const result = await compressor.compress({ stream: Readable.from([data]), options: parcelOptions(), logger: parcelLogger(compressor.name) });
            if (!result) continue;
            const chunks = [];
            for await (const part of result.stream) chunks.push(Buffer.from(part));
            const name = `${shim.name}.${result.type}`;
            bundle[name] = { type: 'asset', fileName: name, name, names: [name], originalFileName: null, originalFileNames: [], source: Buffer.concat(chunks), needsCodeReference: false };
          }
        }
      }
    },
    async writeBundle(options, bundle) {
      const graph = bundleGraphShim(bundle);
      await reportEvent({ type: 'buildSuccess', bundleGraph: graph, buildTime: Date.now() - startedAt, changedAssets: new Map(), requestBundleGraph: graph });
    },
    async buildEnd(error) {
      if (error) await reportEvent({ type: 'buildFailure', diagnostics: [{ message: error.message ?? String(error) }] });
    },
    async closeBundle() {},
  };
};

const blobText = async (blob) => {
  if (typeof blob === 'string') return blob;
  if (Buffer.isBuffer(blob)) return blob.toString('utf8');
  const parts = [];
  for await (const part of blob) parts.push(Buffer.from(part));
  return Buffer.concat(parts).toString('utf8');
};

// ---- the packages Parcel plugins import ---------------------------------------------------------

const parcelBase = (kind) => ({ [kind]: class { constructor(implementation) { this[PARCEL_PLUGIN] = implementation; } } })[kind];
const parcelPluginShim = Object.fromEntries(['Transformer', 'Resolver', 'Reporter', 'Optimizer', 'Packager', 'Namer', 'Runtime', 'Bundler', 'Validator', 'Compressor'].map((k) => [k, parcelBase(k)]));

const parcelUtilsShim = {
  blobToString: blobText,
  blobToBuffer: async (blob) => (Buffer.isBuffer(blob) ? blob : Buffer.from(await blobText(blob))),
  relativePath: (from, to, leadingDotSlash = true) => {
    const rel = path.relative(from, to).split(path.sep).join('/');
    return leadingDotSlash && !rel.startsWith('.') ? './' + rel : rel;
  },
  normalizeSeparators: (p) => p.replace(/\\/g, '/'),
  isGlob: (p) => /[*?{}[\]]/.test(p),
  globToRegex: parcelGlob,
  md5FromString: (s) => crypto.createHash('md5').update(s).digest('hex'),
  md5FromObject: (o) => crypto.createHash('md5').update(JSON.stringify(o)).digest('hex'),
  hashString: (s) => crypto.createHash('md5').update(s).digest('hex'),
  hashBuffer: (b) => crypto.createHash('md5').update(b).digest('hex'),
  escapeMarkdown: (s) => s,
  countLines: (s) => s.split('\n').length,
  isURL: (s) => /^[a-z][a-z0-9+.-]*:/i.test(s),
  urlJoin: (...a) => a.join('/').replace(/([^:])\/+/g, '$1/'),
  resolveConfig: async (fs_, file, names) => {
    for (let dir = path.dirname(file); ; dir = path.dirname(dir)) {
      for (const n of names) if (fs.existsSync(path.join(dir, n))) return path.join(dir, n);
      if (dir === path.dirname(dir)) return null;
    }
  },
  loadConfig: async (fs_, file, names) => {
    const found = await parcelUtilsShim.resolveConfig(fs_, file, names);
    return found ? { config: parseLenientJson(fs.readFileSync(found, 'utf8')), files: [{ filePath: found }] } : null;
  },
  PromiseQueue: class { constructor() { this.q = []; } add(fn) { const p = Promise.resolve().then(fn); this.q.push(p); return p; } run() { return Promise.all(this.q); } },
  debounce: (fn) => fn,
  throttle: (fn) => fn,
  objectSortedEntries: (o) => Object.entries(o).sort(([a], [b]) => (a < b ? -1 : 1)),
  generateCertificate() {},
  prettyDiagnostic: (d) => ({ message: diagnosticText(d), codeframe: '', stack: '', hints: [], documentation: '' }),
  setDifference: (a, b) => new Set([...a].filter((x) => !b.has(x))),
};

const parcelDiagnosticShim = {
  default: class ThrowableDiagnostic extends Error {
    constructor(opts) {
      const diagnostics = toArray(opts?.diagnostic ?? opts?.diagnostics ?? opts);
      super(diagnostics.map((d) => d?.message ?? String(d)).join('\n'));
      this.name = 'ThrowableDiagnostic';
      this.diagnostics = diagnostics;
    }
  },
  errorToDiagnostic: (error) => ({ message: error?.message ?? String(error), stack: error?.stack, origin: 'rathkit-pack' }),
  md: (strings, ...values) => String.raw({ raw: strings }, ...values),
  escapeMarkdown: (s) => s,
  generateJSONCodeHighlights: () => [],
  encodeJSONKeyComponent: (s) => s,
};
parcelDiagnosticShim.ThrowableDiagnostic = parcelDiagnosticShim.default;
