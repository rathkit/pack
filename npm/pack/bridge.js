'use strict';
// Plugin objects given to `build()` / `serve()`: the plugin host is a Node process of the program's, so the objects, which live in this
// process, are reached over a local socket. The host gets a generated config file with one stand-in plugin for each object, whose hooks
// call this process and wait for the answer.
//
// What crosses is JSON (bytes as base64). A hook gets a `this` with the context methods that can be asynchronous: `resolve`, `load`,
// `warn`, `info`, `debug`, `error`, `addWatchFile`. The ones that answer at once and need the build's own data (`emitFile`, `parse`,
// `getModuleInfo`, ...) are not there, and say so; a plugin that needs them goes in a config file, where it runs in the host itself.
// `generateBundle` and `writeBundle` get the output files with their text, and what the plugin changes in `code` and `source`, or removes, is
// applied.
const net = require('node:net');
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
        if (!['resolve', 'load', 'warn', 'info', 'debug', 'addWatchFile'].includes(message.method)) throw new Error('this.' + message.method + ' is not available to plugin objects passed to the API');
        // JSON turns an argument that was not given (undefined) into null.
        answer = { result: await context[message.method](...message.args.map((a) => (a === null ? undefined : a))) };
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
  const sockets = new Set();
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    socket.on('error', () => {});
    let buffer = '';
    let authorized = false;
    const waiting = new Map();
    let requests = 0;
    const send = (message) => socket.write(encode(message) + '\n');
    const contextFor = (callId, meta) => {
      const ask = (method, args, wait) =>
        new Promise((done, fail) => {
          const rid = wait ? ++requests : 0;
          if (wait) waiting.set(rid, { done, fail });
          send({ t: 'ctx', call: callId, rid, method, args });
          if (!wait) done();
        });
      const unavailable = (name) => () => {
        throw new Error(`this.${name} is not available to a plugin object passed to the API; put the plugin in a config file`);
      };
      const message = (m) => (typeof m === 'function' ? m() : m && typeof m === 'object' ? m.message ?? String(m) : m);
      const base = {
        meta,
        resolve: (source, importer, options) => ask('resolve', [source, importer, options], true),
        load: (options) => ask('load', [options], true),
        warn: (m) => ask('warn', [message(m)], false),
        info: (m) => ask('info', [message(m)], false),
        debug: (m) => ask('debug', [message(m)], false),
        addWatchFile: (file) => ask('addWatchFile', [file], false),
        error: (m) => {
          throw Object.assign(new Error(message(m)), typeof m === 'object' && m ? { code: m.code, loc: m.loc, id: m.id } : {});
        },
      };
      return new Proxy(base, {
        get(target, key) {
          if (key in target || typeof key === 'symbol' || key === 'then' || key === 'toJSON') return target[key];
          return unavailable(String(key));
        },
      });
    };
    const handle = async (message) => {
      if (message.t === 'ctxret') {
        const entry = waiting.get(message.rid);
        waiting.delete(message.rid);
        if (entry) message.error ? entry.fail(new Error(message.error.message)) : entry.done(message.result);
        return;
      }
      if (message.t !== 'call') return;
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
    socket.on('data', (chunk) => {
      buffer += chunk;
      let at;
      while ((at = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, at);
        buffer = buffer.slice(at + 1);
        const message = decode(line);
        if (!authorized) {
          if (message.token !== token) return socket.destroy();
          authorized = true;
          continue;
        }
        handle(message);
      }
    });
  });
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  const dir = mkdtempSync(join(tmpdir(), 'rkp-plugins-'));
  const file = join(dir, 'rkp-plugin-objects.mjs');
  const info = { port: server.address().port, token, items: descriptions, base: base ? resolve(cwd ?? process.cwd(), base) : null, command, mode };
  // The info is in the file as a template string (a JSON text with no backquote or ${ in it, which JSON.stringify keeps out of the way).
  const text = JSON.stringify(info).replace(/\\/g, '\\\\').replace(/`/g, '\\`').replace(/\$\{/g, '\\${');
  writeFileSync(file, HOST_SIDE.replace('__INFO__', () => text));
  return {
    config: file,
    close() {
      for (const socket of sockets) socket.destroy();
      server.close();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

module.exports = { startBridge, flatten };
