// The dev server side of the plugin host: Vite's server hooks for `rkp serve`.
//
// This file is appended to host.mjs and shares its scope. The HTTP server itself is in Rust; what lives here is
// what plugins see of it: `configureServer(server)` with connect-style `server.middlewares`, `server.ws`,
// `server.watcher`; `transformIndexHtml`; and `handleHotUpdate`. Rust asks for each request in turn.

const dev = { server: null, middlewares: [], post: [], listeners: new Map(), watcherListeners: new Map(), options: null };

const makeEmitter = () => {
  const listeners = new Map();
  const emitter = {
    on(event, fn) {
      if (!listeners.has(event)) listeners.set(event, []);
      listeners.get(event).push(fn);
      return emitter;
    },
    off(event, fn) {
      listeners.set(event, (listeners.get(event) ?? []).filter((f) => f !== fn));
      return emitter;
    },
    once(event, fn) {
      const wrapper = (...a) => (emitter.off(event, wrapper), fn(...a));
      return emitter.on(event, wrapper);
    },
    emit(event, ...args) {
      for (const fn of listeners.get(event) ?? []) fn(...args);
      return (listeners.get(event) ?? []).length > 0;
    },
  };
  emitter.addListener = emitter.on;
  emitter.removeListener = emitter.off;
  return emitter;
};

/** A connect-style stack: `use(fn)` or `use('/prefix', fn)`; handlers are `(req, res, next)`. */
const makeMiddlewares = () => {
  const stack = [];
  const app = (req, res, done) => {
    let i = 0;
    const next = (error) => {
      const layer = stack[i++];
      if (!layer) return done(error);
      if (layer.route) {
        const url = req.url ?? '/';
        const pathname = url.split('?')[0];
        if (pathname !== layer.route && !pathname.startsWith(layer.route.endsWith('/') ? layer.route : layer.route + '/')) return next(error);
        const original = req.url;
        req.url = url.slice(layer.route.length) || '/';
        if (!req.url.startsWith('/')) req.url = '/' + req.url;
        const inner = (e) => {
          req.url = original;
          next(e);
        };
        return call(layer.handle, error, req, res, inner);
      }
      call(layer.handle, error, req, res, next);
    };
    const call = (handle, error, request, response, nextFn) => {
      try {
        if (error) {
          if (handle.length === 4) handle(error, request, response, nextFn);
          else nextFn(error);
        } else if (handle.length === 4) nextFn();
        else {
          const out = handle(request, response, nextFn);
          if (out && typeof out.then === 'function') out.catch(nextFn);
        }
      } catch (e) {
        nextFn(e);
      }
    };
    next();
  };
  app.use = (route, handle) => {
    if (typeof route === 'function') stack.push({ route: null, handle: route });
    else stack.push({ route: route.replace(/\/$/, '') || '/', handle });
    return app;
  };
  app.stack = stack;
  return app;
};

const makeRequest = ({ method, url, headers, body }) => {
  const req = Readable.from(body && body.length ? [Buffer.from(body, 'base64')] : []);
  Object.assign(req, { method, url, originalUrl: url, headers: Object.fromEntries(Object.entries(headers ?? {}).map(([k, v]) => [k.toLowerCase(), v])), httpVersion: '1.1', socket: { remoteAddress: '127.0.0.1', encrypted: false }, connection: { remoteAddress: '127.0.0.1' } });
  return req;
};

const makeResponse = (finish) => {
  const headers = new Map();
  const chunks = [];
  let ended = false;
  const res = makeEmitter();
  Object.assign(res, {
    statusCode: 200,
    statusMessage: '',
    headersSent: false,
    get writableEnded() {
      return ended;
    },
    finished: false,
    setHeader(name, value) {
      headers.set(String(name).toLowerCase(), Array.isArray(value) ? value.join(', ') : String(value));
      return res;
    },
    getHeader: (name) => headers.get(String(name).toLowerCase()),
    getHeaders: () => Object.fromEntries(headers),
    hasHeader: (name) => headers.has(String(name).toLowerCase()),
    removeHeader: (name) => headers.delete(String(name).toLowerCase()),
    writeHead(status, a, b) {
      res.statusCode = status;
      const extra = typeof a === 'object' && a ? a : b;
      for (const [k, v] of Object.entries(extra ?? {})) res.setHeader(k, v);
      res.headersSent = true;
      return res;
    },
    flushHeaders() {
      res.headersSent = true;
    },
    write(chunk, encoding, cb) {
      if (chunk !== undefined && chunk !== null) chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk), typeof encoding === 'string' ? encoding : 'utf8'));
      res.headersSent = true;
      if (typeof encoding === 'function') encoding();
      else if (typeof cb === 'function') cb();
      return true;
    },
    end(chunk, encoding, cb) {
      if (ended) return res;
      if (typeof chunk === 'function') chunk = undefined;
      if (chunk !== undefined && chunk !== null) res.write(chunk, encoding);
      ended = true;
      res.finished = true;
      finish({ status: res.statusCode, headers: Object.fromEntries(headers), body: Buffer.concat(chunks) });
      res.emit('finish');
      res.emit('close');
      if (typeof encoding === 'function') encoding();
      else if (typeof cb === 'function') cb();
      return res;
    },
    on: res.on,
  });
  return res;
};

const ensureServer = () => {
  if (dev.server) return dev.server;
  const middlewares = makeMiddlewares();
  const watcher = makeEmitter();
  watcher.add = () => watcher;
  watcher.unwatch = () => watcher;
  watcher.close = async () => {};
  watcher.getWatched = () => ({});
  const ws = makeEmitter();
  const messageListeners = new Map();
  ws.send = (event, data) => {
    const payload = typeof event === 'object' ? event : { type: 'custom', event, data };
    send({ event: 'ws', payload: JSON.stringify(payload) });
  };
  ws.on = (event, fn) => {
    if (!messageListeners.has(event)) messageListeners.set(event, []);
    messageListeners.get(event).push(fn);
    return ws;
  };
  ws.off = (event, fn) => (messageListeners.set(event, (messageListeners.get(event) ?? []).filter((f) => f !== fn)), ws);
  ws.clients = new Set();
  ws.close = async () => {};
  dev.messageListeners = messageListeners;
  const moduleGraph = {
    urlToModuleMap: new Map(),
    idToModuleMap: new Map(),
    getModuleById: (id) => moduleGraph.idToModuleMap.get(id),
    getModulesByFile: (file) => (moduleGraph.idToModuleMap.has(file) ? new Set([moduleGraph.idToModuleMap.get(file)]) : undefined),
    getModuleByUrl: async (url) => moduleGraph.urlToModuleMap.get(url),
    invalidateModule() {},
    invalidateAll() {},
    ensureEntryFromUrl: async (url) => ({ id: url, url }),
  };
  const config = viteConfig();
  dev.server = {
    middlewares,
    watcher,
    ws,
    hot: ws,
    moduleGraph,
    config,
    httpServer: null,
    resolvedUrls: { local: [], network: [] },
    root,
    transformIndexHtml: async (url, html) => transformIndexHtml(html, url),
    transformRequest: async () => null,
    ssrLoadModule: async () => {
      throw new Error('server.ssrLoadModule() is not supported');
    },
    close: async () => {},
    listen: async () => dev.server,
    restart: async () => {},
    reloadModule: async () => {},
    printUrls() {},
    bindCLIShortcuts() {},
    waitForRequestsIdle: async () => {},
    pluginContainer: { resolveId: async (id, importer) => resolveId(id, importer, { kind: 'import-statement' }, null) },
  };
  return dev.server;
};

/** `configureServer`: plugins add middlewares; a function they return runs after the built-in ones. */
async function configureServer({ port, host }) {
  const server = ensureServer();
  server.config.server = { port, host, middlewareMode: false };
  for (const plugin of plugins) {
    const fn = handlerOf(plugin.configureServer);
    if (!fn) continue;
    const post = await callHook(plugin, 'configureServer', [server]);
    if (typeof post === 'function') dev.post.push(post);
  }
  server.resolvedUrls = { local: [`http://${host === '0.0.0.0' ? 'localhost' : host}:${port}/`], network: [] };
  for (const post of dev.post) await post();
  return { middlewares: server.middlewares.stack.length > 0 };
}

/** One request through the plugins' middlewares: `{ handled, status, headers, body }`. */
async function serveRequest(request) {
  const server = ensureServer();
  if (!server.middlewares.stack.length) return { handled: false };
  return new Promise((resolve) => {
    const req = makeRequest(request);
    const res = makeResponse((out) => resolve({ handled: true, status: out.status, headers: out.headers, body: out.body.toString('base64') }));
    server.middlewares(req, res, (error) => {
      if (res.writableEnded) return;
      if (error) {
        log('error', `[server] ${error?.message ?? error}`);
        resolve({ handled: true, status: 500, headers: { 'content-type': 'text/plain' }, body: Buffer.from(String(error?.stack ?? error)).toString('base64') });
      } else resolve({ handled: false });
    });
  });
}

const renderTag = ({ tag, attrs = {}, children }) => {
  const attributes = Object.entries(attrs)
    .filter(([, v]) => v !== false && v !== undefined)
    .map(([k, v]) => (v === true ? k : `${k}="${String(v).replace(/"/g, '&quot;')}"`))
    .join(' ');
  const open = `<${tag}${attributes ? ' ' + attributes : ''}>`;
  if (['link', 'meta', 'base', 'img', 'br', 'hr', 'input'].includes(tag)) return open;
  const inner = Array.isArray(children) ? children.map(renderTag).join('') : children ?? '';
  return `${open}${inner}</${tag}>`;
};

const injectTags = (html, tags) => {
  const place = (list, text, where) => {
    if (!list.length) return html;
    const block = list.map((t) => '  ' + renderTag(t)).join('\n') + '\n';
    const [open, close] = where.startsWith('head') ? [/<head[^>]*>/i, /<\/head>/i] : [/<body[^>]*>/i, /<\/body>/i];
    if (where.endsWith('-prepend')) return open.test(html) ? html.replace(open, (m) => m + '\n' + block) : block + html;
    return close.test(html) ? html.replace(close, (m) => block + m) : html + block;
  };
  for (const where of ['head-prepend', 'head', 'body-prepend', 'body']) {
    html = place(tags.filter((t) => (t.injectTo ?? 'head-prepend') === where), '', where);
  }
  return html;
};

/** Vite's `transformIndexHtml`: functions, `{ order, handler }`, results as a string, `{ html, tags }` or a list of tags. */
async function transformIndexHtml(html, url = '/', built = false) {
  const hooks = plugins
    .map((p) => ({ plugin: p, hook: p.transformIndexHtml }))
    .filter((h) => h.hook)
    .map((h) => ({ ...h, order: typeof h.hook === 'object' ? h.hook.order ?? h.hook.enforce : undefined, fn: typeof h.hook === 'function' ? h.hook : h.hook.handler ?? h.hook.transform }))
    .filter((h) => typeof h.fn === 'function');
  const sorted = [...hooks.filter((h) => h.order === 'pre'), ...hooks.filter((h) => !h.order || (h.order !== 'pre' && h.order !== 'post')), ...hooks.filter((h) => h.order === 'post')];
  const ctx = { path: url, filename: path.join(root, url.split('?')[0].replace(/^\//, '') || 'index.html'), server: dev.server ?? undefined, originalUrl: url };
  // After a build the hooks see what was built, as in Vite: every file of the output, and the page's entry chunk.
  if (built && lastBundle) {
    ctx.bundle = lastBundle;
    ctx.chunk = Object.values(lastBundle).find((f) => f.type === 'chunk' && f.isEntry);
  }
  const pending = [];
  for (const { plugin, fn } of sorted) {
    const result = await fn.call(createContext(plugin), html, ctx);
    if (result === undefined || result === null) continue;
    if (typeof result === 'string') html = result;
    else if (Array.isArray(result)) pending.push(...result);
    else {
      if (typeof result.html === 'string') html = result.html;
      pending.push(...(result.tags ?? []));
    }
  }
  return pending.length ? injectTags(html, pending) : html;
}

/** A file changed: plugins' `handleHotUpdate` may change which modules update, or stop the page from reloading. */
async function hotUpdate({ file, event }) {
  const server = ensureServer();
  server.watcher.emit(event === 'delete' ? 'unlink' : event === 'create' ? 'add' : 'change', file);
  let suppressed = false;
  const rel = '/' + path.relative(root, file).split(path.sep).join('/');
  const module = { id: file, file, url: rel, type: file.endsWith('.css') ? 'css' : 'js', importers: new Set(), importedModules: new Set(), isSelfAccepting: false };
  let modules = [module];
  for (const plugin of plugins) {
    const fn = handlerOf(plugin.handleHotUpdate);
    if (!fn) continue;
    const result = await callHook(plugin, 'handleHotUpdate', [{ file, timestamp: Date.now(), modules, read: () => fs.promises.readFile(file, 'utf8'), server }]);
    if (Array.isArray(result)) {
      modules = result;
      if (result.length === 0) suppressed = true;
    }
  }
  return { suppressed };
}

/** A message the page sent through `import.meta.hot.send`. */
async function wsMessage({ raw }) {
  const { event, data } = JSON.parse(raw);
  const server = ensureServer();
  const client = { send: (e, d) => server.ws.send(e, d) };
  for (const fn of dev.messageListeners?.get(event) ?? []) await fn(data, client);
  return null;
}

Object.assign(handlers, {
  configureServer,
  serveRequest,
  hotUpdate,
  wsMessage,
  transformIndexHtml: ({ html, path: url, built }) => transformIndexHtml(html, url, !!built),
});
