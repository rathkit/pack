'use strict';
// Plugin objects given to `build()` / `serve()`: the plugin host is a Node process of the program's, so the objects, which live in this
// process, are reached over a local socket. The host gets a generated config file with one stand-in plugin for each object, whose hooks
// call this process and wait for the answer.
//
// What crosses is JSON (bytes as base64). A hook gets a `this` with the build's context methods. `resolve` and `load` are asynchronous, as in
// Rollup. The ones that answer at once (`emitFile`, `getFileName`, `setAssetSource`, `parse`, `getModuleInfo`, `getModuleIds`,
// `getWatchFiles`) have to block the hook until the build has answered, so the socket is held by a worker thread and the hook waits on
// a shared flag (`Atomics.wait`) while the worker asks. `generateBundle` and `writeBundle` get the output files with their text, and what
// the plugin changes in `code` and `source`, or removes, is applied.
const { Worker, MessageChannel, receiveMessageOnPort } = require('node:worker_threads');
const crypto = require('node:crypto');
const { mkdtempSync, writeFileSync, rmSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { tmpdir } = require('node:os');

const HOOKS = ['options', 'resolveId', 'resolveDynamicImport', 'load', 'transform', 'moduleParsed', 'buildStart', 'buildEnd', 'renderChunk', 'generateBundle', 'writeBundle', 'closeBundle', 'closeWatcher', 'watchChange', 'resolveFileUrl', 'renderStart', 'banner', 'footer', 'intro', 'outro', 'config', 'configResolved', 'transformIndexHtml', 'handleHotUpdate'];
const UNSUPPORTED = ['configureServer', 'configurePreviewServer', 'buildApplication', 'setup'];

const encode = (value) =>
  JSON.stringify(value, (key, v) => {
    if (v instanceof Uint8Array) return { __bytes: Buffer.from(v).toString('base64') };
    if (v && v.type === 'Buffer' && Array.isArray(v.data)) return { __bytes: Buffer.from(v.data).toString('base64') };
    if (typeof v === 'bigint') return Number(v);
    if (typeof v === 'function') return undefined;
    return v;
  });
const decode = (text) => JSON.parse(text, (key, v) => (v && typeof v === 'object' && typeof v.__bytes === 'string' ? Buffer.from(v.__bytes, 'base64') : v));

// The generated config, run by the plugin host. Everything that varies is in `info`.
const HOST_SIDE = `
import net from 'node:net';
import { pathToFileURL } from 'node:url';
const info = JSON.parse(process.env.RKP_BRIDGE_INFO ?? ${'`'}__INFO__${'`'});
const encode = ${encode.toString()};
const decode = ${decode.toString()};
const contexts = new Map();
const waiting = new Map();
let calls = 0;
let buffer = '';
const socket = net.connect(info.port, '127.0.0.1');
const ready = new Promise((resolve, reject) => {
  socket.once('connect', () => {
    socket.write(JSON.stringify({ token: info.token }) + '\\n');
    resolve();
  });
  socket.once('error', reject);
  socket.on('error', () => {});
});
socket.on('close', () => {
  for (const { reject } of waiting.values()) reject(new Error('the process that holds the plugin objects ended'));
  waiting.clear();
});
socket.on('data', async (chunk) => {
  buffer += chunk;
  let at;
  while ((at = buffer.indexOf('\\n')) >= 0) {
    const line = buffer.slice(0, at);
    buffer = buffer.slice(at + 1);
    const message = decode(line);
    if (message.t === 'ret') {
      const entry = waiting.get(message.id);
      waiting.delete(message.id);
      if (!entry) continue;
      if (message.error) entry.reject(Object.assign(new Error(message.error.message), { plugin: message.error.plugin, code: message.error.code, loc: message.error.loc, id: message.error.id }));
      else entry.resolve(message);
    } else if (message.t === 'ctx') {
      const context = contexts.get(message.call);
      let answer;
      try {
        if (!context) throw new Error('the hook has ended');
        if (!['resolve', 'load', 'warn', 'info', 'debug', 'addWatchFile', 'emitFile', 'getFileName', 'setAssetSource', 'parse', 'getModuleInfo', 'getModuleIds', 'getWatchFiles'].includes(message.method)) throw new Error('this.' + message.method + ' is not available to plugin objects passed to the API');
        // JSON turns an argument that was not given (undefined) into null.
        let value = await context[message.method](...message.args.map((a) => (a === null ? undefined : a)));
        if (message.method === 'getModuleIds') value = Array.from(value);
        answer = { result: value };
      } catch (error) {
        answer = { error: { message: error.message } };
      }
      if (message.rid) socket.write(encode({ t: 'ctxret', rid: message.rid, ...answer }) + '\\n');
    }
  }
});
async function call(plugin, hook, args, context) {
  await ready;
  if (socket.destroyed) throw new Error('the process that holds the plugin objects ended');
  const id = ++calls;
  contexts.set(id, context);
  const reply = new Promise((resolve, reject) => waiting.set(id, { resolve, reject }));
  const bundle = hook === 'generateBundle' || hook === 'writeBundle' ? args[1] : undefined;
  socket.write(encode({ t: 'call', id, plugin, hook, args: bundle ? [args[0], bundle] : args, meta: context?.meta }) + '\\n');
  try {
    const answer = await reply;
    if (bundle && answer.bundle) {
      for (const key of Object.keys(bundle)) if (!(key in answer.bundle)) delete bundle[key];
      for (const [key, value] of Object.entries(answer.bundle)) {
        if (bundle[key]) {
          if ('code' in value) bundle[key].code = value.code;
          if ('source' in value) bundle[key].source = value.source;
        } else bundle[key] = value;
      }
    }
    return answer.result;
  } finally {
    contexts.delete(id);
  }
}
let base = {};
if (info.base) {
  const loaded = await import(pathToFileURL(info.base).href);
  base = loaded.default ?? loaded;
  if (typeof base === 'function') base = await base({ command: info.command, mode: info.mode }, {});
  base = (await base) ?? {};
  if (Array.isArray(base)) base = base[0] ?? {};
}
const flat = (value) => (Array.isArray(value) ? value.flatMap(flat) : value ? [value] : []);
const bridges = [];
for (const item of info.items) {
  if (item.kind === 'module') {
    // A plugin made by a module, in this process: the module's export, called with the options if it is a function.
    const loaded = await import(pathToFileURL(item.module).href);
    let value = loaded[item.export ?? 'default'];
    if (value === undefined) throw new Error('rkp: ' + item.module + ' has no export ' + (item.export ?? 'default'));
    if (typeof value === 'function') value = await value(item.options);
    bridges.push(...flat(await value));
    continue;
  }
  const plugin = { name: item.name };
  if (item.enforce) plugin.enforce = item.enforce;
  if (item.apply) plugin.apply = item.apply;
  for (const [hook, shape] of Object.entries(item.hooks)) {
    const forward = async function (...args) {
      if (hook === 'config' || hook === 'configResolved') args = [{ ...args[0], plugins: undefined }, ...args.slice(1)];
      return call(item.index, hook, args, this);
    };
    plugin[hook] = shape ? { ...shape, handler: forward } : forward;
  }
  bridges.push(plugin);
}
export default { ...base, plugins: [...(base.plugins ?? []), ...bridges] };
`;

/** Whether an item of `plugins` is a description of a module to load (`{ module, export, options }`) rather than a plugin. */
const isModule = (item) => item && typeof item === 'object' && typeof item.module === 'string' && !HOOKS.some((h) => typeof item[h] === 'function' || typeof item[h]?.handler === 'function');

/** Describes the plugins for the host: for a plugin object, the hooks it has and their ordering (it throws for ones that cannot work over
 * the bridge); for a module, where it is. */
function describe(plugins, cwd) {
  return plugins.map((plugin, index) => {
    if (isModule(plugin)) return { kind: 'module', module: resolve(cwd ?? process.cwd(), plugin.module), export: plugin.export, options: plugin.options };
    const description = { kind: 'object', index, name: String(plugin.name ?? `plugin-${index}`), hooks: {} };
    for (const hook of UNSUPPORTED) {
      if (plugin[hook] !== undefined) throw new Error(`rkp: the plugin ${description.name} has ${hook}, which needs the build's own objects; put it in a config file instead of passing the object`);
    }
    if (plugin.enforce) description.enforce = plugin.enforce;
    if (typeof plugin.apply === 'string') description.apply = plugin.apply;
    for (const hook of HOOKS) {
      const value = plugin[hook];
      if (typeof value === 'function') description.hooks[hook] = null;
      else if (value && typeof value.handler === 'function') description.hooks[hook] = { order: value.order, sequential: value.sequential };
    }
    return description;
  });
}

async function flatten(list) {
  const out = [];
  for (const item of await Promise.all([].concat(list))) {
    if (Array.isArray(item)) out.push(...(await flatten(item)));
    else if (item) out.push(item);
  }
  return out;
}

// The thread that holds the socket. It passes the host's calls to the main thread, writes what the main thread sends, and answers the main
// thread's questions to the host: asynchronously by a message, or, for a question the main thread is blocked on, through a port and a flag.
const WORKER = `
const { parentPort, workerData } = require('node:worker_threads');
const net = require('node:net');
const encode = ${encode.toString()};
const decode = ${decode.toString()};
const { token, flag: flagBuffer, port } = workerData;
const flag = new Int32Array(flagBuffer);
const blocked = new Set();
let socket = null;
const server = net.createServer((client) => {
  if (socket) return client.destroy();
  let buffer = '';
  let authorized = false;
  client.on('error', () => {});
  client.on('close', () => {
    if (socket === client) socket = null;
  });
  client.on('data', (chunk) => {
    buffer += chunk;
    let at;
    while ((at = buffer.indexOf('\\n')) >= 0) {
      const line = buffer.slice(0, at);
      buffer = buffer.slice(at + 1);
      const message = decode(line);
      if (!authorized) {
        if (message.token !== token) return client.destroy();
        authorized = true;
        socket = client;
        continue;
      }
      if (message.t === 'ctxret' && blocked.delete(message.rid)) {
        port.postMessage(message.error ? { error: message.error.message } : { result: message.result });
        Atomics.store(flag, 0, 1);
        Atomics.notify(flag, 0);
      } else if (message.t === 'ctxret') parentPort.postMessage({ t: 'answer', rid: message.rid, result: message.result, error: message.error && message.error.message });
      else parentPort.postMessage(message);
    }
  });
});
parentPort.on('message', (m) => {
  if (m.t === 'send') {
    if (socket) socket.write(encode(m.message) + '\\n');
  } else if (m.t === 'ask') {
    if (m.sync) blocked.add(m.rid);
    if (!socket) {
      if (m.sync) {
        blocked.delete(m.rid);
        port.postMessage({ error: 'the build has gone' });
        Atomics.store(flag, 0, 1);
        Atomics.notify(flag, 0);
      }
      return;
    }
    socket.write(encode({ t: 'ctx', call: m.call, rid: m.rid, method: m.method, args: m.args }) + '\\n');
  } else if (m.t === 'close') {
    if (socket) socket.destroy();
    server.close();
    process.exit(0);
  }
});
server.listen(0, '127.0.0.1', () => parentPort.postMessage({ t: 'listening', port: server.address().port }));
`;

// The context methods that answer at once, which the hook blocks for.
const BLOCKING = ['emitFile', 'getFileName', 'setAssetSource', 'parse', 'getModuleInfo', 'getModuleIds', 'getWatchFiles'];

/**
 * Starts the socket for these plugin objects and writes the config the host loads. `base` is the config file the build would have used,
 * which the generated one continues. Returns the config's path and a way to end it all.
 */
async function startBridge(list, { base, cwd, command, mode }) {
  const plugins = await flatten(list);
  const descriptions = describe(plugins, cwd);
  const token = crypto.randomBytes(16).toString('hex');
  const hooks = (index, hook) => {
    const value = plugins[index][hook];
    return typeof value === 'function' ? value : value?.handler;
  };
  const flag = new Int32Array(new SharedArrayBuffer(4));
  const { port1, port2 } = new MessageChannel();
  const worker = new Worker(WORKER, { eval: true, workerData: { token, flag: flag.buffer, port: port2 }, transferList: [port2] });
  const waiting = new Map();
  let requests = 0;
  const send = (message) => worker.postMessage({ t: 'send', message });
  // A question to the host that a hook can wait for: an answer is a message.
  const askLater = (call, method, args) =>
    new Promise((done, fail) => {
      const rid = ++requests;
      waiting.set(rid, { done, fail });
      worker.postMessage({ t: 'ask', rid, call, method, args });
    });
  const askNow = (call, method, args) => {
    worker.postMessage({ t: 'ask', rid: ++requests, call, method, args, sync: true });
    Atomics.store(flag, 0, 0);
    Atomics.wait(flag, 0, 0);
    const answer = receiveMessageOnPort(port1)?.message;
    if (!answer || answer.error) throw new Error(answer ? answer.error : 'the build did not answer');
    return answer.result;
  };
  const tell = (call, method, args) => worker.postMessage({ t: 'ask', rid: 0, call, method, args });
  const contextFor = (callId, meta) => {
    const message = (m) => (typeof m === 'function' ? m() : m && typeof m === 'object' ? m.message ?? String(m) : m);
    const own = {
      meta,
      resolve: (source, importer, options) => askLater(callId, 'resolve', [source, importer, options]),
      load: (options) => askLater(callId, 'load', [options]),
      warn: (m) => tell(callId, 'warn', [message(m)]),
      info: (m) => tell(callId, 'info', [message(m)]),
      debug: (m) => tell(callId, 'debug', [message(m)]),
      addWatchFile: (file) => tell(callId, 'addWatchFile', [file]),
      error: (m) => {
        throw Object.assign(new Error(message(m)), typeof m === 'object' && m ? { code: m.code, loc: m.loc, id: m.id } : {});
      },
    };
    for (const name of BLOCKING) own[name] = (...args) => askNow(callId, name, args);
    // The build's list of modules is an iterator, as in Rollup.
    own.getModuleIds = (...args) => askNow(callId, 'getModuleIds', args)[Symbol.iterator]();
    return new Proxy(own, {
      get(target, key) {
        if (key in target || typeof key === 'symbol' || key === 'then' || key === 'toJSON') return target[key];
        return () => {
          throw new Error(`this.${String(key)} is not available to a plugin object passed to the API; put the plugin in a config file`);
        };
      },
    });
  };
  const handle = async (message) => {
    const plugin = plugins[message.plugin];
    try {
      const fn = hooks(message.plugin, message.hook);
      const result = await fn.apply(contextFor(message.id, message.meta), message.args);
      const reply = { t: 'ret', id: message.id, result };
      if (message.hook === 'generateBundle' || message.hook === 'writeBundle') {
        reply.bundle = Object.fromEntries(Object.entries(message.args[1] ?? {}).map(([key, value]) => [key, { code: value.code, source: value.source }]));
      }
      send(reply);
    } catch (error) {
      send({ t: 'ret', id: message.id, error: { message: String(error?.message ?? error), plugin: error?.plugin ?? plugin?.name, code: error?.code, loc: error?.loc, id: error?.id } });
    }
  };
  const listening = new Promise((done, fail) => {
    worker.on('message', (m) => {
      if (m.t === 'listening') done(m.port);
      else if (m.t === 'call') handle(m);
      else if (m.t === 'answer') {
        const entry = waiting.get(m.rid);
        waiting.delete(m.rid);
        if (entry) m.error ? entry.fail(new Error(m.error)) : entry.done(m.result);
      }
    });
    worker.once('error', fail);
  });
  const port = await listening;
  const dir = mkdtempSync(join(tmpdir(), 'rkp-plugins-'));
  const file = join(dir, 'rkp-plugin-objects.mjs');
  const info = { port, token, items: descriptions, base: base ? resolve(cwd ?? process.cwd(), base) : null, command, mode };
  // The info is in the file as a template string (a JSON text with no backquote or ${ in it, which JSON.stringify keeps out of the way).
  const text = JSON.stringify(info).replace(/\\/g, '\\\\').replace(/`/g, '\\`').replace(/\$\{/g, '\\${');
  writeFileSync(file, HOST_SIDE.replace('__INFO__', () => text));
  return {
    config: file,
    close() {
      worker.postMessage({ t: 'close' });
      setTimeout(() => worker.terminate(), 200).unref();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

module.exports = { startBridge, flatten };
