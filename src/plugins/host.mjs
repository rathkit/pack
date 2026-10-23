// The plugin host: runs bundler plugins (Rollup/Vite, esbuild, webpack loaders) for rathkit-pack.
//
// rathkit-pack starts this file with Node and talks to it over stdin/stdout, one JSON message per
// line. A message with a `method` is a request and is answered by a message with the same `id` and
// either `result` or `error`; both sides can ask. Everything the plugins print goes to stderr.
//
// Plugins are normalized to one shape, Rollup's: hooks `resolveId`, `load`, `transform`,
// `buildStart`, `buildEnd`, `renderChunk`, `generateBundle`, `writeBundle`, `closeBundle`. An esbuild
// plugin (`setup(build)`) and webpack `module.rules` loaders become plugins of that shape here.
import { pathToFileURL, fileURLToPath } from 'node:url';
import * as nodeModule from 'node:module';

const { createRequire } = nodeModule;

// Node 24 is the oldest release this host supports (`module.registerHooks`, `require()` of ES modules, and the
// rest of what plugins now expect are all there). A clear message beats a failure somewhere inside a plugin.
const MINIMUM_NODE = 24;
if (Number(process.versions.node.split('.')[0]) < MINIMUM_NODE) {
  process.stderr.write(`error: plugins need Node.js ${MINIMUM_NODE} or later, and this is ${process.version}. Install a newer Node, or set RATHKIT_NODE to one.\n`);
  process.exit(1);
}
import os from 'node:os';
import path from 'node:path';
import fs from 'node:fs';
import crypto from 'node:crypto';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { Worker, MessageChannel, receiveMessageOnPort } from 'node:worker_threads';
import { Readable } from 'node:stream';

// ---- transport ---------------------------------------------------------------------------------

const realWrite = process.stdout.write.bind(process.stdout);
process.stdout.write = (...args) => process.stderr.write(...args);
for (const name of ['log', 'info', 'debug']) console[name] = (...args) => console.error(...args);

const send = (message) => realWrite(JSON.stringify(message) + '\n');
const waiting = new Map();
let nextId = 1;
const ask = (method, params) =>
  new Promise((resolve, reject) => {
    const id = `h${nextId++}`;
    waiting.set(id, { resolve, reject });
    send({ id, method, params });
  });
const log = (level, message, extra = {}) => send({ log: level, message: String(message), ...extra });

// ---- state -------------------------------------------------------------------------------------

let root = process.cwd();
let cli = {};
let plugins = [];
let configOptions = {};
const emitted = new Map(); // reference -> { fileName, source, owner }
const emittedChunks = []; // { reference, id, importer, fileName, owner, resolved }
const usedChunkNames = new Set();
const watchDeps = new Map(); // watched file -> ids of the modules that asked for it
let watchMode = false;
const moduleInfo = new Map(); // id -> info
const watchFiles = new Set();
let outputOptionsSeen = {};


const HOOKS = ['resolveId', 'resolveDynamicImport', 'load', 'transform', 'moduleParsed', 'buildStart', 'buildEnd', 'renderChunk', 'generateBundle', 'writeBundle', 'closeBundle', 'closeWatcher', 'watchChange', 'resolveFileUrl', 'renderStart', 'banner', 'footer', 'intro', 'outro'];

const toArray = (v) => (v === undefined || v === null || v === false ? [] : Array.isArray(v) ? v : [v]);
const flatten = (list) => toArray(list).flatMap((item) => (Array.isArray(item) ? flatten(item) : [item])).filter(Boolean);
const handlerOf = (hook) => (typeof hook === 'function' ? hook : hook && typeof hook.handler === 'function' ? hook.handler : null);

// Rollup 4 / Vite 6 hook filters: { id, code } each a string, RegExp, or { include, exclude }.
const matchesFilter = (filter, value) => {
  if (filter === undefined || filter === null) return true;
  const test = (pattern) => (pattern instanceof RegExp ? ((pattern.lastIndex = 0), pattern.test(value)) : typeof pattern === 'string' ? value.includes(pattern) || value === pattern : false);
  if (typeof filter === 'string' || filter instanceof RegExp || Array.isArray(filter)) return toArray(filter).some(test);
  const include = toArray(filter.include);
  const exclude = toArray(filter.exclude);
  if (exclude.some(test)) return false;
  return include.length === 0 || include.some(test);
};

const fileNameOf = (name, source, pattern = 'assets/[name]-[hash][extname]') => {
  const ext = path.extname(name);
  const base = path.basename(name, ext);
  const hash = crypto.createHash('sha256').update(source).digest('hex').slice(0, 8);
  return pattern.replace('[name]', base).replace('[hash]', hash).replace('[extname]', ext).replace('[ext]', ext.slice(1));
};

// ---- the plugin context ------------------------------------------------------------------------

const createContext = (plugin, owner = 'build') => {
  const context = {
    get meta() {
      return { rollupVersion: '4.40.0', watchMode, rathkit: true };
    },
    debug: (m) => log('debug', typeof m === 'function' ? m() : m?.message ?? m),
    info: (m) => log('info', typeof m === 'function' ? m() : m?.message ?? m),
    warn: (m) => log('warn', `[${plugin.name}] ${typeof m === 'function' ? m().message ?? m() : m?.message ?? m}`),
    error: (m) => {
      const e = typeof m === 'string' ? new Error(m) : Object.assign(new Error(m?.message ?? String(m)), m);
      e.plugin = plugin.name;
      throw e;
    },
    resolve: async (source, importer, options = {}) => {
      const skip = options.skipSelf === false ? null : plugin;
      const result = await resolveId(source, importer, { isEntry: !!options.isEntry, kind: options.kind ?? 'import-statement', custom: options.custom, attributes: options.attributes ?? {} }, skip);
      if (!result) return null;
      return { id: result.id, external: result.external ?? false, moduleSideEffects: result.moduleSideEffects ?? true, meta: result.meta ?? {}, syntheticNamedExports: false, attributes: {}, resolvedBy: result.resolvedBy ?? 'rathkit' };
    },
    load: async (options) => {
      const id = typeof options === 'string' ? options : options.id;
      const loaded = await cachedLoad(id);
      let code = loaded?.code ?? null;
      if (code === null && !loaded?.bytes && !id.startsWith('\0') && fs.existsSync(id)) code = await fs.promises.readFile(id, 'utf8');
      let transformed = null;
      if (code !== null) transformed = await cachedTransform(code, id, loaded?.moduleType);
      return moduleInfoFor(id, { code: transformed?.code ?? code });
    },
    emitFile: (file) => {
      if (file.type === 'chunk') {
        // Chunk names are decided now, so `getFileName` and `import.meta.ROLLUP_FILE_URL_` can use them at once.
        // A name made from a virtual module's id (`\0virtual:x`) may hold characters a file name cannot, as in Rollup's default sanitizeFileName.
        const sanitize = (name) => name.replace(/[\u0000-\u001f"#$%&*+,:;<=>?[\]^`{|}\u007f]/g, '_');
        const wanted = sanitize(path.basename(file.fileName ?? `${file.name ?? path.basename(file.id, path.extname(file.id))}.js`));
        if (file.fileName && wanted !== file.fileName) log('warn', `[${plugin.name}] the chunk file name ${file.fileName} is written as ${wanted}: chunks are written flat`);
        let fileName = wanted;
        for (let n = 2; usedChunkNames.has(fileName); n++) fileName = wanted.replace(/(\.js)?$/, `-${n}$1`);
        usedChunkNames.add(fileName);
        const reference = `ref${emitted.size + 1}`;
        emitted.set(reference, { fileName, name: file.name, owner, chunk: true });
        emittedChunks.push({ reference, id: file.id, importer: file.importer, fileName, owner, resolved: null });
        return reference;
      }
      if (file.type === 'prebuilt-chunk') {
        const reference = `ref${emitted.size + 1}`;
        emitted.set(reference, { fileName: file.fileName, source: file.code, name: file.name, owner, prebuilt: true });
        return reference;
      }
      const source = file.source ?? '';
      const fileName = file.fileName ?? fileNameOf(file.name ?? 'asset', source, cli.assetFileNames);
      const reference = `ref${emitted.size + 1}`;
      emitted.set(reference, { fileName, source, name: file.name, owner, explicit: file.fileName !== undefined, needsSource: file.source === undefined });
      return reference;
    },
    setAssetSource: (reference, source) => {
      const file = emitted.get(reference);
      if (file) {
        file.source = source;
        if (file.needsSource && !file.explicit) file.fileName = fileNameOf(file.name ?? 'asset', source, cli.assetFileNames);
      }
    },
    getFileName: (reference) => {
      const file = emitted.get(reference);
      if (!file) throw new Error(`Unknown file reference ${reference}`);
      return file.fileName;
    },
    getModuleInfo: (id) => moduleInfo.get(id) ?? null,
    getModuleIds: () => moduleInfo.keys(),
    addWatchFile: (id) => {
      watchFiles.add(id);
      if (owner !== 'build') {
        if (!watchDeps.has(id)) watchDeps.set(id, new Set());
        watchDeps.get(id).add(owner);
      }
    },
    getWatchFiles: () => [...watchFiles],
    parse: (code, options) => parseModule(code, options),
    cache: { get: () => undefined, set: () => {}, has: () => false, delete: () => false },
    environment: { name: 'client', mode: cli.mode ?? 'production', config: viteConfig() },
  };
  return context;
};

// `this.parse` is synchronous, so it cannot ask the bundler over the message channel; it runs the
// bundler's own parser (`--estree`) as a child process and reads the ESTree it prints.
const parsed = new Map();
// A worker owns one long-lived `rathkit-pack --estree-server` process; the main thread asks it a question and
// waits for the answer with Atomics.wait, so plugins can call `this.parse` as the synchronous function it is.
const SERVER_WORKER = `
const { workerData, parentPort } = require('node:worker_threads');
const { spawn } = require('node:child_process');
const { exe, port, signal } = workerData;
const child = spawn(exe, ['--estree-server'], { stdio: ['pipe', 'pipe', 'inherit'] });
let pending = Buffer.alloc(0);
child.stdout.on('data', (chunk) => {
  pending = Buffer.concat([pending, chunk]);
  const newline = pending.indexOf(10);
  if (newline < 0) return;
  const length = Number(pending.subarray(0, newline).toString());
  if (pending.length < newline + 1 + length) return;
  const text = pending.subarray(newline + 1, newline + 1 + length).toString('utf8');
  pending = pending.subarray(newline + 1 + length);
  port.postMessage(text);
  Atomics.store(signal, 0, 1);
  Atomics.notify(signal, 0);
});
child.on('exit', () => {
  port.postMessage(JSON.stringify({ error: 'the parser process stopped', pos: 0 }));
  Atomics.store(signal, 0, 1);
  Atomics.notify(signal, 0);
});
parentPort.on('message', ({ kind, code }) => {
  const bytes = Buffer.from(code, 'utf8');
  child.stdin.write(kind + ' ' + bytes.length + '\\n');
  child.stdin.write(bytes);
});
`;
let parseServer = null;
/** One question to the parser process: `kind` is 'm' (module first), 's' (script first) or 'g' (free variables). */
const askParser = (kind, code) => {
  const exe = process.env.RATHKIT_EXE;
  if (!exe) throw new Error('this.parse() needs the rathkit-pack program, which started this host');
  if (!parseServer) {
    const signal = new Int32Array(new SharedArrayBuffer(4));
    const { port1, port2 } = new MessageChannel();
    const worker = new Worker(SERVER_WORKER, { eval: true, workerData: { exe, port: port2, signal }, transferList: [port2] });
    worker.unref();
    parseServer = { worker, signal, port: port1 };
  }
  const { worker, signal, port } = parseServer;
  Atomics.store(signal, 0, 0);
  worker.postMessage({ kind, code });
  Atomics.wait(signal, 0, 0);
  return receiveMessageOnPort(port).message;
};

const parseModule = (code, options = {}) => {
  const script = !!options.allowReturnOutsideFunction || options.sourceType === 'script';
  if (options.jsx) throw new Error('this.parse() does not read JSX');
  const key = `${script ? 's' : 'm'}${code}`;
  const hit = parsed.get(key);
  if (hit) return structuredClone(hit);
  let tree;
  try {
    tree = JSON.parse(askParser(script ? 's' : 'm', code));
  } catch (error) {
    throw new Error(`this.parse() failed: ${error.message}`);
  }
  if (tree.error) {
    // Line and column of the failure, as Rollup and acorn report it.
    const before = code.slice(0, tree.pos);
    const line = before.split('\n').length;
    const error = new SyntaxError(`${tree.error} (${line}:${tree.pos - before.lastIndexOf('\n') - 1})`);
    error.pos = tree.pos;
    error.loc = { line, column: tree.pos - before.lastIndexOf('\n') - 1 };
    error.code = 'PARSE_ERROR';
    throw error;
  }
  if (parsed.size > 64) parsed.delete(parsed.keys().next().value);
  parsed.set(key, tree);
  return structuredClone(tree);
};

const viteConfig = () => ({
  root,
  mode: cli.mode ?? 'production',
  command: 'build',
  isProduction: (cli.mode ?? 'production') === 'production',
  base: '/',
  build: { sourcemap: !!cli.sourcemap, minify: !!cli.minify, target: 'esnext', outDir: cli.outdir ?? 'dist', assetsDir: 'assets', lib: false, rollupOptions: {}, ssr: false },
  define: configOptions.define ?? {},
  resolve: { alias: configOptions.alias ?? [], conditions: [], mainFields: [], extensions: ['.mjs', '.js', '.mts', '.ts', '.jsx', '.tsx', '.json'] },
  plugins: [],
  env: { MODE: cli.mode ?? 'production', DEV: false, PROD: true, BASE_URL: '/', SSR: false },
  logger: { info: (m) => log('info', m), warn: (m) => log('warn', m), warnOnce: (m) => log('warn', m), error: (m) => log('error', m), clearScreen() {}, hasWarned: false },
  css: {},
  esbuild: {},
  cacheDir: path.join(root, 'node_modules/.rathkit'),
  publicDir: path.join(root, 'public'),
});

const moduleInfoFor = (id, extra = {}) => {
  const info = { id, code: null, ast: null, isEntry: false, isExternal: false, isIncluded: true, importers: [], dynamicImporters: [], importedIds: [], dynamicallyImportedIds: [], importedIdResolutions: [], dynamicallyImportedIdResolutions: [], meta: {}, moduleSideEffects: true, syntheticNamedExports: false, attributes: {}, exports: [], exportedBindings: null, hasDefaultExport: null, ...(moduleInfo.get(id) ?? {}), ...extra };
  moduleInfo.set(id, info);
  return info;
};

// ---- file references ---------------------------------------------------------------------------

const FILE_URL = /import\.meta\.ROLLUP_(?:FILE|ASSET|CHUNK)_URL_([A-Za-z0-9_$]+)/g;

/** `import.meta.ROLLUP_FILE_URL_<reference>` becomes the address of the emitted file (or what `resolveFileUrl` says). */
const replaceFileUrls = async (code, id) => {
  if (typeof code !== 'string' || !code.includes('import.meta.ROLLUP_')) return code;
  let out = '';
  let last = 0;
  for (const match of code.matchAll(FILE_URL)) {
    const file = emitted.get(match[1]);
    if (!file) throw new Error(`Plugin error - Unable to get file name for unknown file "${match[1]}".`);
    let replacement;
    for (const plugin of hooksOf('resolveFileUrl')) {
      const result = await callHook(plugin, 'resolveFileUrl', [{ chunkId: '', fileName: file.fileName, format: cli.format ?? 'es', moduleId: id, referenceId: match[1], relativePath: file.fileName }]);
      if (result !== null && result !== undefined) {
        replacement = result;
        break;
      }
    }
    replacement ??= `new URL(${JSON.stringify(file.fileName)}, import.meta.url).href`;
    out += code.slice(last, match.index) + replacement;
    last = match.index + match[0].length;
  }
  return out + code.slice(last);
};

// ---- calling hooks -----------------------------------------------------------------------------

const hooksOf = (name) => plugins.filter((p) => handlerOf(p[name]));

const debug = !!process.env.RATHKIT_PLUGIN_DEBUG;
const callHook = async (plugin, name, args, context = createContext(plugin)) => {
  const hook = plugin[name];
  const fn = handlerOf(hook);
  if (debug) console.error(`[host] ${plugin.name}.${name}(${typeof args[0] === 'string' ? JSON.stringify(args[0].slice(0, 70)) : ''}${typeof args[1] === 'string' ? ', ' + JSON.stringify(args[1].slice(-60)) : ''})`);
  try {
    return await fn.apply(context, args);
  } catch (error) {
    if (error && typeof error === 'object') {
      error.plugin ??= plugin.name;
      error.hook ??= name;
    }
    throw error;
  }
};

const resolveCache = new Map();

async function resolveId(source, importer, options, skip) {
  if (options.kind === 'dynamic-import') {
    for (const plugin of plugins) {
      if (plugin === skip || !handlerOf(plugin.resolveDynamicImport)) continue;
      const result = await callHook(plugin, 'resolveDynamicImport', [source, importer]);
      if (result === null || result === undefined) continue;
      if (result === false) return { id: source, external: true, resolvedBy: plugin.name };
      const object = typeof result === 'string' ? { id: result } : result;
      return { ...object, resolvedBy: plugin.name };
    }
  }
  for (const plugin of plugins) {
    if (plugin === skip) continue;
    const hook = plugin.resolveId;
    const fn = handlerOf(hook);
    if (!fn) continue;
    if (hook.filter && !matchesFilter(hook.filter.id, source)) continue;
    const result = await callHook(plugin, 'resolveId', [source, importer, options]);
    if (result === null || result === undefined) continue;
    if (result === false) return { id: source, external: true, resolvedBy: plugin.name };
    const object = typeof result === 'string' ? { id: result } : result;
    if (object.external === 'relative' && importer) object.id = path.resolve(path.dirname(importer), object.id);
    if (object.external === 'absolute' || object.external === 'relative') object.external = true;
    if (object.moduleSideEffects !== undefined || object.meta || object.resolveDir) {
      moduleInfoFor(object.id, { moduleSideEffects: object.moduleSideEffects ?? true, meta: object.meta ?? {} });
    }
    return { ...object, resolvedBy: plugin.name };
  }
  if (options.onlyPlugins) return null;
  // Nobody claimed it: the bundler's own resolution, as Rollup's default.
  const found = await ask('defaultResolve', { source, importer: importer ?? null, kind: options.kind, isEntry: !!options.isEntry });
  if (!found) return null;
  return { ...found, resolvedBy: 'rathkit' };
}

async function loadModule(id) {
  for (const plugin of plugins) {
    const hook = plugin.load;
    if (!handlerOf(hook)) continue;
    if (hook.filter && !matchesFilter(hook.filter.id, id)) continue;
    const result = await callHook(plugin, 'load', [id], createContext(plugin, id));
    if (debug && result) console.error(`[host]   ${JSON.stringify(id.slice(-45))} -> ${JSON.stringify(typeof result === 'string' ? result : result.code ?? '').slice(0, 300)}`);
    if (result === null || result === undefined) continue;
    const object = typeof result === 'string' ? { code: result } : result;
    if (object.moduleSideEffects !== undefined || object.meta) moduleInfoFor(id, { moduleSideEffects: object.moduleSideEffects ?? true, meta: object.meta ?? {} });
    return object;
  }
  return null;
}

// ---- source maps -------------------------------------------------------------------------------

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const B64_INDEX = new Map([...B64].map((c, i) => [c, i]));

const decodeMappings = (mappings) =>
  mappings.split(';').map((line) => {
    const segments = [];
    let column = 0, source = 0, srcLine = 0, srcColumn = 0, name = 0;
    for (const raw of line.split(',')) {
      if (!raw) continue;
      const numbers = [];
      let value = 0, shift = 0;
      for (const ch of raw) {
        const digit = B64_INDEX.get(ch);
        value |= (digit & 31) << shift;
        if (digit & 32) shift += 5;
        else {
          numbers.push(value & 1 ? -(value >> 1) : value >> 1);
          value = 0;
          shift = 0;
        }
      }
      column += numbers[0];
      if (numbers.length === 1) segments.push([column]);
      else {
        source += numbers[1];
        srcLine += numbers[2];
        srcColumn += numbers[3];
        if (numbers.length >= 5) {
          name += numbers[4];
          segments.push([column, source, srcLine, srcColumn, name]);
        } else segments.push([column, source, srcLine, srcColumn]);
      }
    }
    return segments;
  });

const encodeVlq = (n) => {
  let v = n < 0 ? (-n << 1) | 1 : n << 1;
  let out = '';
  do {
    let digit = v & 31;
    v >>>= 5;
    if (v > 0) digit |= 32;
    out += B64[digit];
  } while (v > 0);
  return out;
};

const encodeMappings = (lines) => {
  let source = 0, srcLine = 0, srcColumn = 0, name = 0;
  return lines
    .map((segments) => {
      let column = 0;
      return segments
        .map((s) => {
          let out = encodeVlq(s[0] - column);
          column = s[0];
          if (s.length > 1) {
            out += encodeVlq(s[1] - source) + encodeVlq(s[2] - srcLine) + encodeVlq(s[3] - srcColumn);
            source = s[1];
            srcLine = s[2];
            srcColumn = s[3];
            if (s.length > 4) {
              out += encodeVlq(s[4] - name);
              name = s[4];
            }
          }
          return out;
        })
        .join(',');
    })
    .join(';');
};

const parseMap = (map) => (map === null || map === undefined || map === '' ? null : typeof map === 'string' ? JSON.parse(map) : map);

/** The map from `outer`'s output back through `inner` (the map of the text `outer` read). */
const composeMaps = (outer, inner) => {
  outer = parseMap(outer);
  inner = parseMap(inner);
  if (!inner) return outer;
  if (!outer) return inner;
  const innerLines = decodeMappings(inner.mappings ?? '');
  const sources = [...(inner.sources ?? [])];
  const contents = [...(inner.sourcesContent ?? [])];
  const names = [...(inner.names ?? [])];
  const lines = decodeMappings(outer.mappings ?? '').map((segments) =>
    segments.map((s) => {
      if (s.length < 4) return s;
      // The outer map's source is the inner map's output: find where that came from.
      const candidates = innerLines[s[2]] ?? [];
      let found = null;
      for (const c of candidates) {
        if (c[0] <= s[3]) found = c;
        else break;
      }
      if (!found || found.length < 4) return [s[0]];
      const result = [s[0], found[1], found[2], found[3]];
      if (found.length > 4) result.push(found[4]);
      else if (s.length > 4) {
        const n = outer.names?.[s[4]];
        if (n !== undefined) {
          let at = names.indexOf(n);
          if (at < 0) names.push(n), (at = names.length - 1);
          result.push(at);
        }
      }
      return result;
    }),
  );
  return { version: 3, file: outer.file, sources, sourcesContent: contents.length ? contents : undefined, names, mappings: encodeMappings(lines) };
};

// ---- esbuild plugins ---------------------------------------------------------------------------

const NAMESPACE_PREFIX = '\0ns:';
const idOf = (namespace, p) => (!namespace || namespace === 'file' ? p : `${NAMESPACE_PREFIX}${namespace}:${p}`);
const splitId = (id) => {
  if (id.startsWith(NAMESPACE_PREFIX)) {
    const rest = id.slice(NAMESPACE_PREFIX.length);
    const colon = rest.indexOf(':');
    return { namespace: rest.slice(0, colon), path: rest.slice(colon + 1) };
  }
  return { namespace: 'file', path: id };
};

const esbuildKinds = { 'import-statement': 'import-statement', 'require-call': 'require-call', 'dynamic-import': 'dynamic-import', 'require-resolve': 'require-resolve' };

const adaptEsbuild = async (plugin) => {
  const onStart = [], onEnd = [], onResolve = [], onLoad = [], onDispose = [];
  const pluginData = new Map(); // id -> pluginData from the callback that resolved it
  const resolveDirs = new Map();
  const initialOptions = {
    entryPoints: cli.entries ?? [], bundle: true, outdir: cli.outdir, outfile: cli.outfile, format: cli.format ?? 'esm', platform: cli.platformBrowser ? 'browser' : 'node',
    minify: !!cli.minify, minifyWhitespace: !!cli.minify, minifyIdentifiers: !!cli.minify, minifySyntax: !!cli.minify, sourcemap: !!cli.sourcemap, define: configOptions.define ?? {},
    external: cli.external ?? [], loader: Object.fromEntries((cli.loaders ?? []).map(([e, l]) => [e, l])), plugins: [], absWorkingDir: root, write: true, metafile: false, splitting: !!cli.splitting,
  };
  const build = {
    initialOptions,
    esbuild: { version: '0.25.0' },
    onStart: (cb) => onStart.push(cb),
    onEnd: (cb) => onEnd.push(cb),
    onDispose: (cb) => onDispose.push(cb),
    onResolve: (options, cb) => onResolve.push({ filter: options.filter, namespace: options.namespace, cb }),
    onLoad: (options, cb) => onLoad.push({ filter: options.filter, namespace: options.namespace, cb }),
    resolve: async (p, options = {}) => {
      const result = await resolveId(p, options.importer ?? path.join(options.resolveDir ?? root, 'noop.js'), { isEntry: options.kind === 'entry-point', kind: options.kind ?? 'import-statement' }, adapted);
      if (!result) return { path: '', external: false, errors: [{ text: `Could not resolve "${p}"` }], warnings: [], namespace: '', suffix: '', pluginData: undefined, sideEffects: true, watchFiles: [], watchDirs: [] };
      const split = splitId(result.id);
      return { path: split.path, namespace: split.namespace, external: !!result.external, errors: [], warnings: [], suffix: '', pluginData: pluginData.get(result.id), sideEffects: true, watchFiles: [], watchDirs: [] };
    },
    context: undefined,
  };
  await plugin.setup(build);
  const test = (filter, text) => (filter ? ((filter.lastIndex = 0), filter.test(text)) : true);
  const adapted = {
    name: plugin.name ?? 'esbuild-plugin',
    __onEnd: onEnd,
    __onDispose: onDispose,
    ...(onStart.length && {
      async buildStart() {
        for (const cb of onStart) {
          const result = await cb();
          for (const w of result?.warnings ?? []) log('warn', `[${plugin.name}] ${w.text}`);
          if (result?.errors?.length) throw new Error(`[${plugin.name}] ${result.errors.map((e) => e.text).join('\n')}`);
        }
      },
    }),
    ...(onResolve.length && {
      async resolveId(source, importer, options) {
        const importerSplit = importer ? splitId(importer) : { namespace: 'file', path: '' };
        const resolveDir = importer ? (resolveDirs.get(importer) ?? path.dirname(importerSplit.path)) : root;
        for (const { filter, namespace, cb } of onResolve) {
          if (namespace && namespace !== importerSplit.namespace && importer) continue;
          if (!test(filter, source)) continue;
          const result = await cb({ path: source, importer: importerSplit.path, namespace: importerSplit.namespace, resolveDir, kind: options.isEntry ? 'entry-point' : esbuildKinds[options.kind] ?? 'import-statement', pluginData: importer ? pluginData.get(importer) : undefined, with: options.attributes ?? {} });
          if (!result) continue;
          for (const w of result.warnings ?? []) log('warn', `[${plugin.name}] ${w.text}`);
          if (result.errors?.length) throw new Error(`[${plugin.name}] ${result.errors.map((e) => e.text).join('\n')}`);
          if (result.path === undefined || result.path === '') {
            if (result.external) return { id: source, external: true };
            continue;
          }
          const id = result.external ? result.path : idOf(result.namespace, result.path);
          if (result.pluginData !== undefined) pluginData.set(id, result.pluginData);
          if (result.namespace && result.namespace !== 'file' && importer) resolveDirs.set(id, resolveDir);
          return { id, external: !!result.external, moduleSideEffects: result.sideEffects === false ? false : undefined };
        }
        return null;
      },
    }),
    ...(onLoad.length && {
      async load(id) {
        const { namespace, path: file } = splitId(id);
        for (const { filter, namespace: ns, cb } of onLoad) {
          if (ns && ns !== namespace) continue;
          if (!test(filter, file)) continue;
          const result = await cb({ path: file, namespace, suffix: '', pluginData: pluginData.get(id), with: {} });
          if (!result) continue;
          for (const w of result.warnings ?? []) log('warn', `[${plugin.name}] ${w.text}`);
          if (result.errors?.length) throw new Error(`[${plugin.name}] ${result.errors.map((e) => e.text).join('\n')}`);
          if (result.pluginData !== undefined) pluginData.set(id, result.pluginData);
          if (result.contents === undefined) continue;
          if (result.resolveDir) resolveDirs.set(id, result.resolveDir);
          const bytes = typeof result.contents === 'string' ? null : Buffer.from(result.contents);
          return { code: bytes ? undefined : result.contents, bytes: bytes ? bytes.toString('base64') : undefined, moduleType: result.loader ?? (namespace === 'file' ? undefined : 'js'), resolveDir: result.resolveDir };
        }
        return null;
      },
    }),
    async closeBundle() {
      for (const cb of onEnd) await cb({ errors: [], warnings: [], outputFiles: undefined, metafile: undefined, mangleCache: undefined });
      for (const cb of onDispose) await cb();
    },
  };
  return adapted;
};

// ---- webpack loaders ---------------------------------------------------------------------------

const testRule = (test, resource) => {
  if (test === undefined) return true;
  if (test instanceof RegExp) return ((test.lastIndex = 0), test.test(resource));
  if (typeof test === 'string') return resource.startsWith(test);
  if (typeof test === 'function') return !!test(resource);
  if (Array.isArray(test)) return test.some((t) => testRule(t, resource));
  if (test && typeof test === 'object') {
    if (test.and) return test.and.every((t) => testRule(t, resource));
    if (test.or) return test.or.some((t) => testRule(t, resource));
    if (test.not) return !testRule(test.not, resource);
  }
  return false;
};

const normalizeUse = (rule) => {
  const uses = [];
  const add = (u) => {
    if (typeof u === 'string') {
      // "loader?query" shorthand and "a!b" chains.
      for (const part of u.split('!').filter(Boolean)) {
        const [loader, query] = part.split('?');
        uses.push({ loader, options: query ? Object.fromEntries(new URLSearchParams(query)) : undefined });
      }
    } else if (typeof u === 'function') toArray(u({ resource: '' })).forEach(add);
    else if (u && typeof u === 'object') uses.push({ loader: u.loader, options: u.options });
  };
  toArray(rule.use ?? rule.loader).forEach(add);
  if (rule.loader && rule.options !== undefined && uses.length === 1) uses[0].options = rule.options;
  return uses;
};

const loaderModules = new Map();
const loadLoader = async (request, context) => {
  const key = `${context}\0${request}`;
  if (!loaderModules.has(key)) {
    const req = createRequire(path.join(context, 'noop.js'));
    const file = request.startsWith('.') || path.isAbsolute(request) ? path.resolve(context, request) : req.resolve(request);
    const mod = await import(pathToFileURL(file).href);
    const fn = typeof mod.default === 'function' ? mod.default : typeof mod === 'function' ? mod : mod.default?.default ?? mod;
    // CommonJS loaders: `module.exports = function`, with `.pitch` / `.raw` properties on it.
    const resolved = { fn: typeof fn === 'function' ? fn : mod.default, pitch: mod.pitch ?? fn?.pitch, raw: mod.raw ?? fn?.raw ?? mod.default?.raw, file };
    if (typeof resolved.fn !== 'function') throw new Error(`Loader ${request} does not export a function`);
    loaderModules.set(key, resolved);
  }
  return loaderModules.get(key);
};

// Enough of webpack's compiler and compilation for loaders that read options and make hashes from them.
const hashAdapter = (algorithm = 'sha256') => {
  const name = /^md4$/i.test(algorithm) ? 'sha256' : algorithm; // md4 is gone from modern OpenSSL
  const hash = crypto.createHash(name);
  const wrapper = { update: (data, enc) => (hash.update(data, enc), wrapper), digest: (enc) => hash.digest(enc) };
  return wrapper;
};
const webpackOutput = () => ({ path: path.resolve(root, cli.outdir ?? 'dist'), publicPath: 'auto', filename: '[name].js', hashFunction: 'sha256', hashDigest: 'hex', hashDigestLength: 20, hashSalt: undefined, uniqueName: '', environment: {}, module: false });
const optionsOf = (options) => {
  if (typeof options !== 'string') return options ?? {};
  const text = options.replace(/^\?/, '');
  if (text.startsWith('{')) return JSON.parse(text);
  return Object.fromEntries(new URLSearchParams(text));
};

/** webpack's loader-runner: pitching loaders left to right, then normal loaders right to left. */
const runLoaders = async (resource, query, uses) => {
  const dir = path.dirname(resource);
  const loaders = [];
  for (const u of uses) loaders.push({ ...(await loadLoader(u.loader, root)), options: u.options, request: u.loader, data: {} });
  const dependencies = [];
  const contextDependencies = [];
  const missingDependencies = [];
  const moduleShim = new NormalModule(resource + query);
  moduleShim.loaders = loaders.map((l) => ({ loader: l.file, options: l.options, ident: undefined, type: undefined }));
  const requestOf = (from) => [...loaders.slice(from).map((l) => l.file + (typeof l.options === 'string' ? l.options : '')), resource + query].join('!');
  // What the running loader looks at is `state`; the context is one object for the whole chain, as in webpack,
  // so that what plugins add to it (through NormalModule's `loader` hook) is there for every loader.
  const state = { i: 0, done: null, flags: { async: false } };
  const loaderObjects = loaders.map((l) => ({ path: l.file, request: l.file, query: typeof l.options === 'string' ? l.options : '', options: l.options, ident: undefined, normal: l.fn, pitch: l.pitch, raw: l.raw, data: l.data, pitchExecuted: false, normalExecuted: false }));
  const compilation = getCompilation();
  const warn = (e) => {
    log('warn', `[${loaders[state.i]?.request}] ${e?.message ?? e}`);
    compilation.warnings.push(e instanceof Error ? e : new WebpackError(String(e)));
  };
  const fail = (e) => {
    log('error', `[${loaders[state.i]?.request}] ${e?.message ?? e}`);
    compilation.errors.push(e instanceof Error ? e : new WebpackError(String(e)));
  };
  const context = {
    version: 2, resource: resource + query, resourcePath: resource, resourceQuery: query, resourceFragment: '', context: dir, rootContext: root,
    get request() { return requestOf(state.i); },
    get remainingRequest() { return requestOf(state.i + 1); },
    get currentRequest() { return requestOf(state.i); },
    get previousRequest() { return loaders.slice(0, state.i).map((l) => l.file).join('!'); },
    loaders: loaderObjects,
    mode: cli.mode ?? 'production', target: 'web', sourceMap: !!cli.sourcemap, hot: false,
    get loaderIndex() { return state.i; },
    get data() { return loaders[state.i].data; },
    get query() { return loaders[state.i].options ?? ''; },
    webpack: true,
    _compiler: getCompiler(), _compilation: compilation, _module: moduleShim,
    async: () => ((state.flags.async = true), state.done),
    callback: (...args) => state.done(...args),
    cacheable: () => {},
    getOptions: () => optionsOf(loaders[state.i].options),
    addDependency: (f) => dependencies.push(f),
    addContextDependency: (f) => contextDependencies.push(f),
    addMissingDependency: (f) => missingDependencies.push(f),
    getDependencies: () => dependencies,
    getContextDependencies: () => contextDependencies,
    getMissingDependencies: () => missingDependencies,
    clearDependencies: () => (dependencies.length = 0),
    emitWarning: warn,
    emitError: fail,
    emitFile: (name, source, map, assetInfo) => {
      compilation.emitAsset(name, source instanceof Source ? source : new RawSource(source), assetInfo);
      emitted.set(`ref${emitted.size + 1}`, { fileName: name, source: Buffer.isBuffer(source) ? source : sourceText(source), explicit: true, owner: resource });
    },
    getLogger: (name) => quietLogger(name ?? loaders[state.i].request),
    resolve: (ctx, request, callback) => {
      resolveId(request, path.join(ctx, 'noop.js'), { kind: 'import-statement' }, null).then((r) => callback(null, r?.id), callback);
    },
    getResolve: () => async (ctx, request) => {
      if (process.env.RATHKIT_PLUGIN_DEBUG) console.error('[host] getResolve', JSON.stringify(ctx), JSON.stringify(request));
      return (await resolveId(request, path.join(ctx, 'noop.js'), { kind: 'import-statement' }, null))?.id;
    },
    loadModule: (request, callback) => {
      importModuleText(request, resource).then((m) => callback(null, m.code, null, m), callback);
    },
    importModule: (request, options, callback) => {
      const promise = importModuleValue(request, resource);
      if (typeof options === 'function') callback = options;
      return callback ? void promise.then((v) => callback(null, v), callback) : promise;
    },
    utils: { contextify: (c, r) => contextify(c, r), absolutify: (c, r) => path.resolve(c, r), createHash: hashAdapter },
    fs,
  };
  NormalModule.getCompilationHooks(compilation).loader.call(context, moduleShim);
  const call = (i, fn, args) =>
    new Promise((resolve, reject) => {
      state.i = i;
      state.flags = { async: false };
      let finished = false;
      const done = (err, ...values) => {
        if (finished) return;
        finished = true;
        err ? reject(err) : resolve(values);
      };
      state.done = done;
      try {
        const out = fn.apply(context, args);
        if (state.flags.async) return;
        if (out && typeof out.then === 'function') out.then((v) => done(null, v), done);
        else done(null, out);
      } catch (e) {
        done(e);
      }
    });
  // Pitching: a loader whose pitch returns something ends the descent; the loaders before it run on that.
  let content;
  let start = loaders.length - 1;
  for (let i = 0; i < loaders.length; i++) {
    if (typeof loaders[i].pitch !== 'function') continue;
    loaderObjects[i].pitchExecuted = true;
    const [value] = await call(i, loaders[i].pitch, [requestOf(i + 1), loaders.slice(0, i).map((l) => l.file).join('!'), loaders[i].data]);
    if (value !== undefined) {
      content = typeof value === 'string' || Buffer.isBuffer(value) ? value : String(value);
      start = i - 1;
      break;
    }
  }
  if (content === undefined) content = await fs.promises.readFile(resource);
  for (let i = start; i >= 0; i--) {
    const loader = loaders[i];
    loaderObjects[i].normalExecuted = true;
    const asBuffer = Buffer.isBuffer(content) ? content : Buffer.from(content);
    const input = loader.raw ? asBuffer : asBuffer.toString('utf8');
    const [out] = await call(i, loader.fn, [input, null, undefined]);
    content = out === undefined ? input : out;
  }
  for (const file of dependencies) watchFiles.add(file);
  return { code: Buffer.isBuffer(content) ? content.toString('utf8') : String(content), dependencies };
};

/** `this.loadModule` / `this.importModule`: the module as the build would see it, loaded and transformed. */
const importModuleText = async (request, from) => {
  const resolved = await resolveId(request, from, { kind: 'import-statement' }, null);
  if (!resolved) throw new Error(`Can't resolve '${request}' in '${path.dirname(from)}'`);
  const loaded = await cachedLoad(resolved.id);
  let code = loaded?.code ?? (fs.existsSync(resolved.id.split('?')[0]) ? await fs.promises.readFile(resolved.id.split('?')[0], 'utf8') : '');
  const transformed = await cachedTransform(code, resolved.id, loaded?.moduleType);
  return { id: resolved.id, code: transformed?.code ?? code };
};

/** The exports of a module, made by running it: a small CommonJS evaluation of what the build produced for it. */
const importModuleValue = async (request, from) => {
  const m = await importModuleText(request, from);
  const { code } = await ask('bundleEntry', { entry: m.id, globalName: 'RESULT' });
  const module = { exports: {} };
  new Function('module', 'exports', 'require', '__filename', '__dirname', code)(module, module.exports, createRequire(m.id), m.id.split('?')[0], path.dirname(m.id.split('?')[0]));
  return module.exports;
};

const contextify = (context, request) => {
  const split = (r) => r.split('!').map((part) => {
    const [file, ...q] = part.split('?');
    if (!path.isAbsolute(file)) return part;
    let rel = path.relative(context, file).split(path.sep).join('/');
    if (!rel.startsWith('.')) rel = './' + rel;
    return [rel, ...q].join('?');
  });
  return split(request).join('!');
};

const INLINE_MARK = '__rk=';
const encodeInline = (data) => Buffer.from(JSON.stringify(data)).toString('base64url');
const decodeInline = (text) => JSON.parse(Buffer.from(text, 'base64url').toString('utf8'));

/** `!!css-loader!./a.css` style requests: resolved here, loaded by the loaders it names. */
const resolveInlineRequest = async (request, importer) => {
  // A module made up to be run (see extractStyleSheet) imports relative to the file it was made from.
  if (importer && evalStore.has(importer)) importer = path.join(evalStore.get(importer).dir, 'noop.js');
  const m = /^(!!|-!|!)?(.*)$/s.exec(request);
  const prefix = m[1] ?? '';
  const parts = m[2].split('!');
  const resourceSpec = parts.pop();
  const dir = importer ? path.dirname(importer) : root;
  const loaders = [];
  for (const part of parts.filter(Boolean)) {
    const [spec, ...q] = part.split('?');
    const req = createRequire(path.join(dir, 'noop.js'));
    const file = spec.startsWith('.') || path.isAbsolute(spec) ? path.resolve(dir, spec) : req.resolve(spec);
    loaders.push({ loader: file, options: q.length ? '?' + q.join('?') : undefined });
  }
  const resolved = await resolveId(resourceSpec, importer, { kind: 'import-statement', onlyPlugins: false }, null);
  if (!resolved || resolved.external) return null;
  // `!./x.js`: just the file, no loaders of its own to add.
  if (loaders.length === 0 && prefix) return { id: resolved.id };
  const joiner = resolved.id.includes('?') ? '&' : '?';
  return { id: `${resolved.id}${joiner}${INLINE_MARK}${encodeInline({ prefix, loaders })}` };
};

const ruleMatches = (rule, resource, query) => {
  if (rule.resource !== undefined) return testRule(rule.resource, resource) && (rule.resourceQuery === undefined || testRule(rule.resourceQuery, query));
  if (rule.test === undefined && rule.include === undefined) return false;
  if (rule.test !== undefined && !testRule(rule.test, resource)) return false;
  if (rule.include !== undefined && !testRule(rule.include, resource)) return false;
  if (rule.exclude !== undefined && testRule(rule.exclude, resource)) return false;
  if (rule.resourceQuery !== undefined && !testRule(rule.resourceQuery, query)) return false;
  return true;
};

const evalStore = new Map(); // id -> { code, dir }: modules made up to be run once

/**
 * The output of css-loader (JavaScript that builds a list of style sheet pieces) is run once, and the pieces
 * become a style sheet module of the bundler: the CSS text, and `export default { classes }`.
 */
const extractStyleSheet = async (code, resource) => {
  const id = `\0rk-eval:${evalStore.size + 1}`;
  evalStore.set(id, { code, dir: path.dirname(resource) });
  const built = await ask('bundleEntry', { entry: id, globalName: '' });
  const compilation = getCompilation();
  for (const asset of built.assets) compilation.emitAsset(asset.name, new RawSource(Buffer.from(asset.data, 'base64')));
  const module = { exports: {} };
  new Function('module', 'exports', 'require', '__filename', '__dirname', built.code)(module, module.exports, createRequire(resource), resource, path.dirname(resource));
  const exported = module.exports.default ?? module.exports;
  const items = Array.isArray(exported) ? exported : [];
  const wrap = (item) => {
    let css = String(item[1] ?? '');
    const [, , media, , supports, layer] = item;
    if (media) css = `@media ${media} {\n${css}\n}`;
    if (supports) css = `@supports ${supports} {\n${css}\n}`;
    if (layer !== undefined && layer !== null) css = `@layer${layer ? ' ' + layer : ''} {\n${css}\n}`;
    return css;
  };
  // css-loader keeps class names on the list (`.locals`), or as named exports with `modules.namedExport`.
  let locals = exported.locals ?? module.exports.locals;
  if (!locals) locals = Object.fromEntries(Object.entries(module.exports).filter(([k, v]) => k !== 'default' && k !== '__esModule' && typeof v === 'string'));
  const names = Object.keys(locals);
  const face = [`export default ${JSON.stringify(locals)};`, ...names.filter((n) => /^[A-Za-z_$][\w$]*$/.test(n) && n !== 'default').map((n) => `export var ${n} = ${JSON.stringify(locals[n])};`)].join('\n');
  // css-loader (webpack 5 style) wrote each `url()` as the file URL of the asset; the bundler wants plain paths,
  // relative to the sheet, so that it can copy the files and write their new addresses.
  const relativeUrls = (css) => css.replace(/url\(\s*(["']?)(file:\/\/[^)"']+)\1\s*\)/g, (whole, quote, href) => {
    let file;
    try {
      file = fileURLToPath(href.replace(/[?#].*$/, ''));
    } catch {
      return whole;
    }
    const suffix = (href.match(/[?#].*$/) ?? [''])[0];
    let rel = path.relative(path.dirname(resource), file).split(path.sep).join('/');
    if (!rel.startsWith('.')) rel = './' + rel;
    return `url(${quote}${rel}${suffix}${quote})`;
  });
  return { code: relativeUrls(items.map(wrap).join('\n')), moduleType: 'css', jsFace: names.length ? face : 'export {};' };
};

const adaptWebpackRules = (rules) => {
  const list = flatten(rules).flatMap((r) => (r.oneOf ? r.oneOf.map((o) => ({ ...r, ...o, oneOf: undefined })) : [r]));
  return {
    name: 'webpack-loaders',
    async resolveId(source, importer) {
      if (evalStore.has(source)) return { id: source };
      if (!source.includes('!') || source.startsWith('\0')) return null;
      return resolveInlineRequest(source, importer);
    },
    async load(id) {
      if (evalStore.has(id)) return { code: evalStore.get(id).code, resolveDir: evalStore.get(id).dir, moduleType: 'js' };
      if (id.startsWith('\0')) return null;
      let inline = null;
      const markAt = id.lastIndexOf(INLINE_MARK);
      if (markAt > 0 && (id[markAt - 1] === '?' || id[markAt - 1] === '&')) {
        inline = decodeInline(id.slice(markAt + INLINE_MARK.length));
        id = id.slice(0, markAt - 1);
      }
      const queryAt = id.indexOf('?');
      const resource = queryAt < 0 ? id : id.slice(0, queryAt);
      const query = queryAt < 0 ? '' : id.slice(queryAt);
      if (!path.isAbsolute(resource) || !fs.existsSync(resource)) return null;
      const matched = list.filter((rule) => ruleMatches(rule, resource, query));
      const pre = matched.filter((r) => r.enforce === 'pre'), post = matched.filter((r) => r.enforce === 'post'), normal = matched.filter((r) => !r.enforce);
      let uses;
      if (inline) {
        const mine = inline.loaders;
        if (inline.prefix === '!!') uses = mine;
        else if (inline.prefix === '-!') uses = [...post.flatMap(normalizeUse), ...mine];
        else if (inline.prefix === '!') uses = [...post.flatMap(normalizeUse), ...mine, ...pre.flatMap(normalizeUse)];
        else uses = [...post.flatMap(normalizeUse), ...mine, ...normal.flatMap(normalizeUse), ...pre.flatMap(normalizeUse)];
      } else uses = [...post, ...normal, ...pre].flatMap(normalizeUse);
      if (uses.length === 0) return null;
      // mini-css-extract-plugin's loader needs webpack's module graph; the style sheet goes through this bundler's own instead.
      if (/mini-css-extract-plugin[\\/]dist[\\/]loader\.js$/.test(uses[0].loader)) {
        const rest = uses.slice(1);
        if (rest.length === 0) return { code: await fs.promises.readFile(resource, 'utf8'), moduleType: 'css' };
        const { code } = await runLoaders(resource, query, rest);
        return extractStyleSheet(code, resource);
      }
      const { code } = await runLoaders(resource, query, uses);
      return { code, moduleType: 'js' };
    },
  };
};

// ---- loading the config and the plugins --------------------------------------------------------

const definitionsOf = (definitions, prefix = '') =>
  Object.entries(definitions ?? {}).flatMap(([key, value]) =>
    value && typeof value === 'object' && !(value instanceof RegExp) ? definitionsOf(value, `${prefix}${key}.`) : [[`${prefix}${key}`, typeof value === 'string' ? value : JSON.stringify(value)]],
  );

const externalPlugin = (external) => {
  const tests = toArray(external);
  return {
    name: 'external',
    resolveId(source, importer, options) {
      for (const t of tests) {
        const hit = typeof t === 'function' ? t(source, importer, !!options.isEntry) : t instanceof RegExp ? ((t.lastIndex = 0), t.test(source)) : t === source;
        if (hit) return { id: source, external: true };
      }
      return null;
    },
  };
};

const aliasPlugin = (alias) => {
  const entries = Array.isArray(alias) ? alias : Object.entries(alias ?? {}).map(([find, replacement]) => ({ find, replacement }));
  return {
    name: 'alias',
    async resolveId(source, importer, options) {
      for (const { find, replacement } of entries) {
        let target = null;
        if (find instanceof RegExp) {
          if (find.test(source)) target = source.replace(find, replacement);
        } else if (source === find || source.startsWith(find + '/')) target = replacement + source.slice(find.length);
        if (target === null || target === source) continue;
        const resolved = await this.resolve(target, importer, { ...options, skipSelf: true });
        return resolved ?? { id: target };
      }
      return null;
    },
  };
};

const bannerPlugin = (output) => {
  const text = async (value, chunk) => (typeof value === 'function' ? await value(chunk) : value ?? '');
  return {
    name: 'output-wrappers',
    async renderChunk(code, chunk) {
      const [banner, intro, outro, footer] = await Promise.all([text(output.banner, chunk), text(output.intro, chunk), text(output.outro, chunk), text(output.footer, chunk)]);
      if (!banner && !intro && !outro && !footer) return null;
      const parts = [banner, intro, code, outro, footer].filter((p) => p !== '');
      return { code: parts.join('\n'), map: null };
    },
  };
};

const isWebpackPlugin = (raw) => {
  if (typeof raw === 'function') return true;
  if (!raw || typeof raw !== 'object' || typeof raw.apply !== 'function' || typeof raw.setup === 'function') return false;
  // A class instance with `apply(compiler)`, or a plain object that is not a Rollup/Vite plugin (those have a name or hooks).
  if (Object.getPrototypeOf(raw) !== Object.prototype) return raw.apply.length <= 1;
  return raw.name === undefined && !HOOKS.some((h) => handlerOf(raw[h]));
};

/** The options of a module federation plugin (webpack's, rspack's or Vite's), which the bundler carries out itself. */
let federationOptions = null;
const federationOf = (raw) => {
  if (!raw || typeof raw !== 'object') return null;
  if (raw.__federationOptions) return raw.__federationOptions;
  const options = raw._options;
  if (options && typeof options === 'object' && typeof options.name === 'string' && (raw.constructor?.name === 'ModuleFederationPlugin' || options.exposes || options.remotes || options.shared)) return options;
  return null;
};

const normalizePlugin = async (raw) => {
  if (!raw || (typeof raw !== 'object' && typeof raw !== 'function')) return [];
  const federation = federationOf(raw);
  if (federation) {
    federationOptions = federation;
    return [];
  }
  if (typeof raw === 'object' && typeof raw.setup === 'function' && !HOOKS.some((h) => raw[h])) return [await adaptEsbuild(raw)];
  if (isWebpackPlugin(raw)) {
    // The ones that ship with webpack are done here; the rest apply to the compiler of webpack.mjs.
    const kind = raw?.constructor?.name;
    // Style sheets are extracted by the bundler itself; the plugin only marks that the config wants them in files.
    if (kind === 'MiniCssExtractPlugin') return [];
    if (NOOP_PLUGINS.includes(kind)) return [];
    const builtIn = builtInPlugin(raw);
    wp.plugins.push(builtIn ?? raw);
    return [];
  }
  // Vite's `apply`: only for the build.
  if (raw.apply === 'serve' || (typeof raw.apply === 'function' && !raw.apply({}, { command: 'build', mode: cli.mode ?? 'production' }))) return [];
  return [raw];
};

const orderPlugins = (list) => [...list.filter((p) => p.enforce === 'pre'), ...list.filter((p) => !p.enforce), ...list.filter((p) => p.enforce === 'post')];

async function init(params) {
  if (process.env.RATHKIT_PLUGIN_DEBUG) console.error('[host] init', params.config);
  root = params.root;
  cli = params.cli ?? {};
  installWebpackShims();
  for (const entry of cli.entries ?? []) usedChunkNames.add(`${path.basename(entry, path.extname(entry))}.js`);
  let config = {};
  const parcelrcOnly = params.config && /\.parcelrc$/.test(params.config);
  if (params.config && !parcelrcOnly) {
    const url = pathToFileURL(path.resolve(root, params.config)).href;
    let mod;
    try {
      mod = await import(url);
    } catch (error) {
      throw new Error(`Cannot load ${params.config}: ${error.message}`);
    }
    config = mod.default ?? mod;
    if (typeof config === 'function') config = await config({ command: 'build', mode: cli.mode ?? 'production', isSsrBuild: false }, {});
    config = await config;
    if (Array.isArray(config)) config = config[0] ?? {};
  }
  configOptions = { define: { ...(config.define ?? {}) }, alias: config.resolve?.alias ?? config.alias };
  wp.config = config;
  let list = [];
  for (const raw of flatten(await Promise.all(flatten(config.plugins ?? [])))) list.push(...(await normalizePlugin(raw)));
  list = orderPlugins(list);
  // Vite config hooks may add definitions and aliases.
  for (const plugin of list) {
    if (typeof plugin.config === 'function') {
      const returned = await plugin.config.call(createContext(plugin), { ...config, root }, { command: 'build', mode: cli.mode ?? 'production' });
      if (returned?.define) configOptions.define = { ...configOptions.define, ...returned.define };
      if (returned?.resolve?.alias) configOptions.alias = [...toArray(configOptions.alias ?? []), ...(Array.isArray(returned.resolve.alias) ? returned.resolve.alias : Object.entries(returned.resolve.alias).map(([find, replacement]) => ({ find, replacement })))];
    }
  }
  for (const plugin of list) if (typeof plugin.configResolved === 'function') await plugin.configResolved.call(createContext(plugin), viteConfig());
  // Parcel plugins: a `.parcelrc` (given as the config, or found at the root), or `parcel` in the config.
  const parcelPlugins = [];
  {
    const rc = parcelrcOnly ? path.resolve(root, params.config) : typeof config.parcel === 'string' ? path.resolve(root, config.parcel) : config.parcel === undefined && fs.existsSync(path.join(root, '.parcelrc')) ? path.join(root, '.parcelrc') : null;
    try {
      if (rc || (config.parcel && typeof config.parcel === 'object')) {
        parcel.config = rc ? readParcelConfig(rc) : parcelConfigFrom(config.parcel, root);
        const pipelines = await loadPipelines();
        const any = Object.values(pipelines).some((v) => (Array.isArray(v) ? v.length : Object.keys(v).length));
        if (any) {
          parcel.active = true;
          parcelPlugins.push(parcelAdapter(pipelines));
        }
      }
    } catch (error) {
      throw new Error(`Cannot read the Parcel configuration: ${error.message}`);
    }
  }
  const pre = [];
  if (config.external || config.build?.rollupOptions?.external) pre.push(externalPlugin(config.external ?? config.build.rollupOptions.external));
  if (configOptions.alias && (Array.isArray(configOptions.alias) ? configOptions.alias.length : Object.keys(configOptions.alias).length)) pre.push(aliasPlugin(configOptions.alias));
  const rules = config.module?.rules;
  const post = [];
  // Webpack plugins may ask for modules with inline loaders (`loader.js!./template.html`) even without rules.
  if (rules || wp.plugins.length) post.push(adaptWebpackRules(rules ?? []));
  const output = config.output ? toArray(config.output)[0] : config.build?.rollupOptions?.output ? toArray(config.build.rollupOptions.output)[0] : null;
  if (output && (output.banner || output.footer || output.intro || output.outro)) post.push(bannerPlugin(output));
  // `options` hooks may change the input options: `external`, and the plugin list itself (the CommonJS
  // plugin adds a resolver that way).
  let inputOptions = { input: config.input ?? cli.entries ?? [], external: config.external ?? [], plugins: list };
  const firstExternal = inputOptions.external;
  for (const plugin of [...list]) {
    const fn = handlerOf(plugin.options);
    if (fn) {
      const returned = await fn.call(createContext(plugin), inputOptions);
      if (returned && typeof returned === 'object') inputOptions = returned;
    }
  }
  list = flatten(inputOptions.plugins);
  const externals = inputOptions.external !== firstExternal ? toArray(inputOptions.external) : [];
  // Plugins written for webpack's compiler get one together; they are applied once, before the first build.
  const compilerPlugins = [];
  if (wp.plugins.length) {
    wp.active = true;
    await wpInit();
    compilerPlugins.push(wpAdapter());
    if (wp.provide) compilerPlugins.push(provideAdapter());
  }
  plugins = [...(externals.length ? [externalPlugin(externals)] : []), ...pre, ...compilerPlugins, ...parcelPlugins, ...list, ...post];
  const hooks = {};
  for (const name of HOOKS) hooks[name] = hooksOf(name).length > 0;
  hooks.transform = hooksOf('transform').length > 0;
  const input = config.input ?? config.build?.rollupOptions?.input ?? config.build?.lib?.entry;
  federationOptions ??= config.federation ?? config.moduleFederation ?? null;
  return {
    // Functions and promises cannot be carried out of the host; what is left is plain data.
    federation: federationOptions ? JSON.parse(JSON.stringify(federationOptions, (key, value) => (typeof value === 'function' || value instanceof Promise ? undefined : value))) : null,
    plugins: plugins.map((p) => p.name ?? 'anonymous'),
    hooks,
    define: Object.entries(configOptions.define).map(([k, v]) => [k, typeof v === 'string' ? v : JSON.stringify(v)]),
    input: input ? (typeof input === 'string' ? [input] : Array.isArray(input) ? input : Object.values(input)) : [],
  };
}

// ---- requests from rathkit-pack ----------------------------------------------------------------

const encodeCode = (code) => (typeof code === 'string' ? code : code === undefined || code === null ? '' : Buffer.from(code).toString('utf8'));


// Plugins may ask for a module (this.load) before the bundler gets to it; both see the same work.
const loadCache = new Map();
const cachedLoad = (id) => {
  if (!loadCache.has(id)) loadCache.set(id, loadModule(id));
  return loadCache.get(id);
};
const transformCache = new Map();
const cachedTransform = (code, id, moduleType) => {
  moduleType = moduleType ?? null;
  const cached = transformCache.get(id);
  if (cached && cached.code === code && cached.moduleType === moduleType) return cached.promise;
  const promise = transformModule(code, id, moduleType);
  transformCache.set(id, { code, moduleType, promise });
  return promise;
};

async function transformModule(code, id, moduleType) {
  let current = code;
  let changed = false;
  const maps = [];
  let type = moduleType;
  for (const plugin of plugins) {
    const hook = plugin.transform;
    if (!handlerOf(hook)) continue;
    if (hook.filter && (!matchesFilter(hook.filter.id, id) || !matchesFilter(hook.filter.code, current))) continue;
    const context = createContext(plugin, id);
    context.getCombinedSourcemap = () => composeAll(maps) ?? { version: 3, sources: [id], names: [], mappings: '' };
    const result = await callHook(plugin, 'transform', [current, id, { ssr: false, moduleType: type }], context);
    if (result === null || result === undefined) continue;
    const object = typeof result === 'string' ? { code: result } : result;
    if (debug) console.error(`[host]   transform ${JSON.stringify(id.slice(-45))} => ${JSON.stringify(object.code ?? null)?.slice(0, 500)} meta=${JSON.stringify(object.meta)?.slice(0, 200)}`);
    if (object.meta) moduleInfoFor(id, { meta: { ...(moduleInfo.get(id)?.meta ?? {}), ...object.meta } });
    if (object.moduleSideEffects !== undefined) moduleInfoFor(id, { moduleSideEffects: object.moduleSideEffects });
    if (object.code === undefined || object.code === null) continue;
    current = encodeCode(object.code);
    changed = true;
    if (object.map) maps.push(parseMap(object.map));
    else maps.push(null);
    if (object.moduleType) type = object.moduleType;
  }
  const withUrls = await replaceFileUrls(current, id);
  if (withUrls !== current) {
    current = withUrls;
    changed = true;
    maps.push(null);
  }
  if (!changed) return null;
  const map = composeAll(maps);
  return { code: current, map: map ? JSON.stringify(map) : undefined, moduleType: type };
}

let lastBundle = {};

const handlers = {
  init,

  async buildStart() {
    // A new build forgets what the last one's own hooks emitted; what its modules emitted stays with their cached results.
    for (const [reference, file] of emitted) if (file.owner === 'build' || file.owner === 'generate') emitted.delete(reference);
    for (let i = emittedChunks.length - 1; i >= 0; i--) if (emittedChunks[i].owner === 'build') emittedChunks.splice(i, 1);
    for (const plugin of hooksOf('buildStart')) await callHook(plugin, 'buildStart', [{ input: cli.entries ?? [], plugins }]);
    return null;
  },

  async buildEnd({ error }) {
    for (const plugin of hooksOf('buildEnd')) await callHook(plugin, 'buildEnd', error ? [new Error(error)] : []);
    return null;
  },

  /** Chunks plugins asked for, with the module each starts from resolved. */
  async emittedChunks() {
    const list = [];
    for (const chunk of emittedChunks) {
      if (!chunk.resolved) {
        const found = await resolveId(chunk.id, chunk.importer, { isEntry: true, kind: 'import-statement' }, null);
        if (!found) throw new Error(`Could not resolve entry module "${chunk.id}".`);
        if (found.external) {
          log('warn', `the chunk for "${chunk.id}" was left out: the module is external`);
          chunk.resolved = { id: null };
          continue;
        }
        chunk.resolved = { id: found.id };
        moduleInfoFor(found.id, { isEntry: true });
      }
      if (chunk.resolved.id) list.push({ reference: chunk.reference, id: chunk.resolved.id, fileName: chunk.fileName });
    }
    return list;
  },

  /** The shape of the finished graph: who imports whom. `moduleParsed` runs for every module. */
  async moduleGraph({ modules }) {
    const known = new Set(modules.map((m) => m.id));
    const externals = new Set();
    for (const m of modules) for (const id of [...m.importedIds, ...m.dynamicallyImportedIds]) if (!known.has(id)) externals.add(id);
    const resolution = (id) => ({ id, external: externals.has(id), moduleSideEffects: true, meta: moduleInfo.get(id)?.meta ?? {}, syntheticNamedExports: false, attributes: {}, resolvedBy: 'rathkit' });
    for (const m of modules) {
      const t = transformCache.get(m.id);
      const code = (t && (await t.promise)?.code) ?? (await loadCache.get(m.id))?.code ?? null;
      moduleInfoFor(m.id, {
        id: m.id,
        code: typeof code === 'string' ? code : null,
        isEntry: m.isEntry || (moduleInfo.get(m.id)?.isEntry ?? false),
        importedIds: m.importedIds,
        dynamicallyImportedIds: m.dynamicallyImportedIds,
        importedIdResolutions: m.importedIds.map(resolution),
        dynamicallyImportedIdResolutions: m.dynamicallyImportedIds.map(resolution),
        importers: m.importers,
        dynamicImporters: m.dynamicImporters,
        exports: m.exports,
        exportedBindings: { '.': m.exports },
        hasDefaultExport: m.esm ? m.hasDefaultExport : null,
        moduleSideEffects: m.sideEffects,
        isIncluded: true,
      });
    }
    for (const id of externals) moduleInfoFor(id, { id, isExternal: true, isEntry: false, importers: modules.filter((m) => m.importedIds.includes(id)).map((m) => m.id), dynamicImporters: modules.filter((m) => m.dynamicallyImportedIds.includes(id)).map((m) => m.id), hasDefaultExport: null });
    if (wp.active) await wpModules(modules);
    for (const m of modules) for (const plugin of hooksOf('moduleParsed')) await callHook(plugin, 'moduleParsed', [moduleInfo.get(m.id)]);
    return null;
  },

  async watchFiles() {
    return [...watchFiles, ...(wp.active ? [...wp.compilation.fileDependencies] : [])];
  },

  async watchMode({ on }) {
    watchMode = !!on;
    return null;
  },

  /** Changed files: what was cached about them (and about the modules that watch them) is dropped. */
  async watchChange({ changes }) {
    for (const { id, event } of changes) {
      const affected = new Set([id, ...(watchDeps.get(id) ?? [])]);
      for (const one of affected) {
        loadCache.delete(one);
        transformCache.delete(one);
        for (const [reference, file] of emitted) if (file.owner === one) emitted.delete(reference);
        for (let i = emittedChunks.length - 1; i >= 0; i--) if (emittedChunks[i].owner === one) emittedChunks.splice(i, 1);
        moduleInfo.delete(one);
      }
      for (const plugin of hooksOf('watchChange')) await callHook(plugin, 'watchChange', [id, { event }]);
    }
    return null;
  },

  async closeWatcher() {
    for (const plugin of hooksOf('closeWatcher')) await callHook(plugin, 'closeWatcher', []);
    return null;
  },

  async resolveId({ source, importer, isEntry, kind, attributes, onlyPlugins }) {
    const result = await resolveId(source, importer ?? undefined, { isEntry: !!isEntry, kind: kind ?? 'import-statement', attributes: attributes ?? {}, custom: {}, onlyPlugins: !!onlyPlugins }, null);
    if (!result) return null;
    if (isEntry) moduleInfoFor(result.id, { isEntry: true });
    // A result from the bundler's own resolution has no plugin behind it.
    return { id: result.id, external: !!result.external, byPlugin: result.resolvedBy !== 'rathkit', sideEffects: result.moduleSideEffects };
  },

  async load({ id }) {
    const result = await cachedLoad(id);
    if (!result) return null;
    const info = moduleInfo.get(id);
    const code = result.bytes === undefined ? await replaceFileUrls(encodeCode(result.code), id) : undefined;
    return {
      code,
      bytes: result.bytes,
      map: result.map ? JSON.stringify(parseMap(result.map)) : undefined,
      moduleType: result.moduleType,
      resolveDir: result.resolveDir,
      jsFace: result.jsFace,
      sideEffects: info?.moduleSideEffects === false ? false : undefined,
    };
  },

  async transform({ code, id, moduleType }) {
    return cachedTransform(code, id, moduleType);
  },

  async renderChunk({ code, chunk, options, map }) {
    let current = code;
    let combined = parseMap(map);
    let changed = false;
    for (const plugin of hooksOf('renderChunk')) {
      const result = await callHook(plugin, 'renderChunk', [current, chunk, options, { chunks: {} }]);
      if (result === null || result === undefined) continue;
      const object = typeof result === 'string' ? { code: result } : result;
      current = encodeCode(object.code);
      changed = true;
      if (object.map) combined = composeMaps(object.map, combined);
      else if (object.map === null) combined = null;
      else combined = null;
    }
    if (!changed) return null;
    return { code: current, map: combined ? JSON.stringify(combined) : undefined, mapDropped: !combined && !!map };
  },

  async generateBundle({ files, options }) {
    const bundle = {};
    for (const f of files) {
      bundle[f.fileName] =
        f.type === 'chunk'
          ? { importedBindings: {}, referencedFiles: [], implicitlyLoadedBefore: [], modules: {}, moduleIds: [], isDynamicEntry: false, isImplicitEntry: false, facadeModuleId: null, exports: [], imports: [], dynamicImports: [], ...(f.chunk ?? {}), type: 'chunk', fileName: f.fileName, name: f.chunk?.name ?? f.name, code: f.code, map: f.map ? parseMap(f.map) : null, isEntry: !!f.isEntry, preliminaryFileName: f.fileName, sourcemapFileName: f.map ? `${f.fileName}.map` : null }
          : { type: 'asset', fileName: f.fileName, name: f.name, names: f.name ? [f.name] : [], originalFileName: null, originalFileNames: [], source: f.encoding === 'base64' ? Buffer.from(f.source, 'base64') : f.source, needsCodeReference: false };
    }
    const before = new Map(Object.entries(bundle).map(([k, v]) => [k, v.type === 'chunk' ? v.code : v.source]));
    for (const plugin of hooksOf('generateBundle')) {
      const context = createContext(plugin);
      context.emitFile = (file) => {
        if (file.type === 'chunk') return createContext(plugin, 'generate').emitFile(file);
        const source = file.source ?? '';
        const fileName = file.fileName ?? fileNameOf(file.name ?? 'asset', source, cli.assetFileNames);
        bundle[fileName] = { type: 'asset', fileName, name: file.name, names: file.name ? [file.name] : [], originalFileName: null, originalFileNames: [], source, needsCodeReference: false };
        return fileName;
      };
      await callHook(plugin, 'generateBundle', [options, bundle, true], context);
    }
    lastBundle = bundle;
    // Files plugins emitted earlier (this.emitFile) join the output.
    for (const file of emitted.values()) {
      if (file.source !== undefined && !(file.fileName in bundle) && !before.has(file.fileName)) bundle[file.fileName] = { type: 'asset', fileName: file.fileName, name: file.name, source: file.source };
    }
    const result = [];
    for (const [fileName, item] of Object.entries(bundle)) {
      if (item.type === 'chunk') {
        if (!before.has(fileName) || before.get(fileName) !== item.code || item.map !== undefined) result.push({ fileName, type: 'chunk', code: item.code, changed: before.get(fileName) !== item.code, map: item.map ? JSON.stringify(parseMap(item.map)) : undefined });
      } else {
        const source = item.source;
        const same = before.has(fileName) && String(before.get(fileName)) === String(source);
        if (!same) {
          const binary = typeof source !== 'string';
          result.push({ fileName, type: 'asset', source: binary ? Buffer.from(source).toString('base64') : source, encoding: binary ? 'base64' : 'utf8', changed: true });
        }
      }
    }
    return { files: result, deleted: files.map((f) => f.fileName).filter((n) => !(n in bundle)) };
  },

  async writeBundle({ options, names }) {
    // The files as the last generateBundle left them; names from before that (a plugin renamed them) are skipped.
    const bundle = Object.fromEntries(names.map((n) => [n, lastBundle[n] ?? { type: 'chunk', fileName: n, code: '' }]));
    for (const plugin of hooksOf('writeBundle')) await callHook(plugin, 'writeBundle', [options, bundle]);
    return null;
  },

  async closeBundle() {
    for (const plugin of hooksOf('closeBundle')) await callHook(plugin, 'closeBundle', []);
    return null;
  },

  async shutdown() {
    setImmediate(() => process.exit(0));
    return null;
  },
};

const composeAll = (maps) => {
  // maps[i] is the map plugin i made, from its output back to its input; the first one is closest to the source.
  let combined = null;
  for (const map of maps) {
    if (map === null) {
      combined = null; // that step gave no map: nothing before it can be traced
      continue;
    }
    combined = combined ? composeMaps(map, combined) : map;
  }
  return combined;
};

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', (line) => {
  if (!line) return;
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  if ('method' in message) {
    const handler = handlers[message.method];
    Promise.resolve()
      .then(() => {
        if (!handler) throw new Error(`unknown request ${message.method}`);
        return handler(message.params ?? {});
      })
      .then((result) => send({ id: message.id, result: result ?? null }))
      .catch((error) => send({ id: message.id, error: { message: error?.message ?? String(error), stack: error?.stack, plugin: error?.plugin, hook: error?.hook, id: error?.id, loc: error?.loc } }));
  } else {
    const slot = waiting.get(message.id);
    if (!slot) return;
    waiting.delete(message.id);
    if (message.error) slot.reject(new Error(message.error.message ?? message.error));
    else slot.resolve(message.result);
  }
});
rl.on('close', () => process.exit(0));
process.on('unhandledRejection', (error) => log('error', `unhandled rejection in a plugin: ${error?.message ?? error}`));
send({ ready: true });
