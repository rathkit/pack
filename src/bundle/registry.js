// The module registry of `rkp serve --hmr`: every module is a function in a table, run on first use, and a changed module can be
// replaced in a running page. The page's own client (src/serve/client.js) hands it the server's update messages.
const __r = (() => {
  const defs = Object.create(null);
  const kinds = Object.create(null);
  const deps = Object.create(null);
  const specs = Object.create(null);
  const cache = Object.create(null);
  const holders = Object.create(null);
  const interop = Object.create(null);
  const importers = Object.create(null);
  const hotObjects = Object.create(null);
  const hotData = Object.create(null);
  const accepts = Object.create(null);
  const disposers = Object.create(null);
  const declined = Object.create(null);
  const tag = typeof Symbol === 'function' ? Symbol.toStringTag : '@@toStringTag';
  const client = (id) => globalThis.__rathkit_hot && globalThis.__rathkit_hot(id);

  const link = () => {
    for (const id in importers) delete importers[id];
    for (const id in deps) for (const dep of deps[id]) (importers[dep] ||= new Set()).add(id);
  };
  const namespace = () => {
    const object = Object.create(null);
    Object.defineProperty(object, tag, { value: 'Module' });
    return object;
  };

  // Runs a module the first time it is asked for, and returns its record. A module that is being run is returned as it is
  // (a cycle sees what has been exported so far).
  const load = (id) => {
    const found = cache[id];
    if (found) return found;
    const fn = defs[id];
    if (!fn) throw new Error('Cannot find module ' + id);
    const esm = kinds[id] === 'esm';
    // The exports object of an ES module is the same one every time it runs, so what imported it keeps seeing the new bindings.
    const exports = esm ? (holders[id] ||= namespace()) : {};
    if (esm) for (const key of Reflect.ownKeys(exports)) if (key !== tag) delete exports[key];
    const module = { id, exports, loaded: false };
    cache[id] = module;
    delete accepts[id];
    delete declined[id];
    let result;
    try {
      result = fn.call(esm ? undefined : exports, exports, module, api, (spec) => {
        const target = (specs[id] || {})[spec];
        if (target === undefined) {
          // A module of Node itself, when this runs in Node.
          const builtin = typeof process !== 'undefined' && process.getBuiltinModule && process.getBuiltinModule(spec);
          if (builtin) return builtin;
          throw new Error("Cannot find module '" + spec + "'");
        }
        return api.r(target);
      });
    } catch (error) {
      delete cache[id];
      throw error;
    }
    // A module with top-level await is an async function: its record holds the promise of its having run.
    if (result && typeof result.then === 'function') {
      module.promise = result.then(
        () => {
          module.loaded = true;
        },
        (error) => {
          if (cache[id] === module) delete cache[id];
          throw error;
        },
      );
    } else module.loaded = true;
    return module;
  };
  const settled = async (id) => {
    const module = load(id);
    if (module.promise) await module.promise;
    return module;
  };

  // `import x from` a CommonJS module: the default is `module.exports` (unless it is an ES module's), and the rest its properties,
  // read when used.
  const view = (id) => {
    load(id);
    return (interop[id] ||= new Proxy(Object.create(null), {
      get(_, key) {
        const value = cache[id] && cache[id].exports;
        if (key === 'default') return value && value.__esModule ? value.default : value;
        if (key === tag) return 'Module';
        return value == null ? undefined : value[key];
      },
      has(_, key) {
        const value = cache[id] && cache[id].exports;
        return key === 'default' || (value != null && key in Object(value));
      },
      ownKeys() {
        const value = cache[id] && cache[id].exports;
        return ['default', ...(value == null ? [] : Reflect.ownKeys(Object(value)).filter((k) => k !== 'default'))];
      },
      getOwnPropertyDescriptor(target, key) {
        const value = cache[id] && cache[id].exports;
        if (key !== 'default' && (value == null || !(key in Object(value)))) return undefined;
        return { enumerable: true, configurable: true, writable: false, value: api.get(id, key) };
      },
    }));
  };

  const hot = (id) =>
    (hotObjects[id] ||= {
      get data() {
        return (hotData[id] ||= {});
      },
      accept(target, callback) {
        const entry = (accepts[id] ||= { self: false, callbacks: [], deps: Object.create(null) });
        if (target === undefined || typeof target === 'function') {
          entry.self = true;
          if (target) entry.callbacks.push(target);
          return;
        }
        const list = Array.isArray(target) ? target : [target];
        list.forEach((spec) => {
          const id2 = (specs[id] || {})[spec];
          if (id2 !== undefined) (entry.deps[id2] ||= []).push({ callback, list, many: Array.isArray(target) });
        });
      },
      acceptExports(_names, callback) {
        this.accept(callback);
      },
      dispose(callback) {
        (disposers[id] ||= []).push(callback);
      },
      prune() {},
      decline() {
        declined[id] = true;
      },
      invalidate() {
        Promise.resolve(propagate([id])).then((ok) => ok || location.reload());
      },
      on(event, callback) {
        const c = client(id);
        if (c) c.on(event, callback);
      },
      off(event, callback) {
        const c = client(id);
        if (c) c.off(event, callback);
      },
      send(event, data) {
        const c = client(id);
        if (c) c.send(event, data);
      },
    });

  // The modules that must run again for the changed ones to take effect, up to the ones that accept the change. False when a change
  // reaches an entry that nothing accepts, and the page has to reload.
  const propagate = (changed) => {
    const rerun = new Set();
    const boundaries = [];
    const walk = (id, seen) => {
      if (!cache[id]) return true;
      if (declined[id]) return false;
      const accepted = accepts[id];
      if (accepted && accepted.self) {
        rerun.add(id);
        boundaries.push({ module: id, self: true, callbacks: accepted.callbacks.slice() });
        return true;
      }
      const parents = importers[id];
      if (!parents || !parents.size || seen.has(id)) return false;
      rerun.add(id);
      const next = new Set(seen).add(id);
      for (const parent of parents) {
        if (!cache[parent]) continue;
        const wanted = accepts[parent] && accepts[parent].deps[id];
        if (wanted) {
          boundaries.push({ module: parent, dep: id, wanted: wanted.slice() });
          continue;
        }
        if (!walk(parent, next)) return false;
      }
      return true;
    };
    for (const id of changed) if (!walk(id, new Set())) return false;
    for (const id of rerun) {
      const list = disposers[id];
      delete disposers[id];
      for (const dispose of list || []) {
        try {
          dispose((hotData[id] ||= {}));
        } catch (error) {
          console.error('[rkp] dispose failed in ' + id, error);
        }
      }
    }
    for (const id of rerun) delete cache[id];
    const finish = () => {
      for (const b of boundaries) {
        if (b.self) for (const callback of b.callbacks) callback(cache[b.module] && cache[b.module].exports);
        else
          for (const { callback, list, many } of b.wanted) {
            if (!callback) continue;
            callback(many ? list.map((spec) => (((specs[b.module] || {})[spec] === b.dep) ? cache[b.dep].exports : undefined)) : cache[b.dep].exports);
          }
      }
      console.log('[rkp] hot updated: ' + [...rerun].join(', '));
      return true;
    };
    const failed = (error) => {
      console.error('[rkp] hot update failed; reloading', error);
      return false;
    };
    // Modules with top-level await take time: the answer is then a promise, and the callbacks wait for them.
    try {
      const waiting = [];
      for (const b of boundaries) {
        const module = load(b.self ? b.module : b.dep);
        if (module.promise) waiting.push(module.promise);
      }
      if (waiting.length) return Promise.all(waiting).then(finish).catch(failed);
      return finish();
    } catch (error) {
      return failed(error);
    }
  };

  const api = {
    def(id, kind, fn, dependencies, table) {
      defs[id] = fn;
      kinds[id] = kind;
      deps[id] = dependencies;
      specs[id] = table;
    },
    i: (id) => load(id).exports,
    c: view,
    r: (id) => (kinds[id] === 'esm' ? load(id).exports : load(id).exports),
    d: (id) =>
      Promise.resolve().then(() => {
        const module = load(id);
        const done = () => (kinds[id] === 'esm' ? module.exports : view(id));
        return module.promise ? module.promise.then(done) : done();
      }),
    // An import of a module that may have to wait: ready when it has run.
    w: (id) => settled(id).then((m) => m.exports),
    get: (id, key) => {
      const value = cache[id] && cache[id].exports;
      return key === 'default' ? (value && value.__esModule ? value.default : value) : value == null ? undefined : value[key];
    },
    e(exports, getters) {
      for (const key in getters) Object.defineProperty(exports, key, { enumerable: true, configurable: true, get: getters[key] });
    },
    s(exports, source) {
      for (const key of Object.keys(source)) {
        if (key !== 'default' && !(key in exports)) Object.defineProperty(exports, key, { enumerable: true, configurable: true, get: () => source[key] });
      }
    },
    h: hot,
    run(entries) {
      link();
      const waiting = [];
      for (const id of entries) {
        const module = load(id);
        if (module.promise) waiting.push(module.promise);
      }
      if (waiting.length) return Promise.all(waiting);
    },
    // A message from the server: the modules that changed (their new text, imports and the specifiers they use), and the ones gone.
    update(message) {
      const changed = [];
      for (const id of Object.keys(message.modules || {})) {
        const m = message.modules[id];
        let fn;
        try {
          fn = (0, eval)('(' + m.code + '\n)\n//# sourceURL=rkp-module:' + encodeURI(id));
        } catch (error) {
          console.error('[rkp] cannot read the new ' + id, error);
          return false;
        }
        api.def(id, m.kind, fn, m.deps, m.specs);
        changed.push(id);
      }
      for (const id of message.removed || []) {
        delete defs[id];
        delete kinds[id];
        delete deps[id];
        delete specs[id];
        delete cache[id];
      }
      link();
      return propagate(changed);
    },
  };
  globalThis.__rathkit_hmr = api;
  return api;
})();
