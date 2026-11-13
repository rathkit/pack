// Prints what a module exports, for tools/run-corpus.mjs to compare a library built by rkp with the one its authors publish.
//   node --experimental-import-meta-resolve probe-exports.mjs shape-esm|shape-cjs|format-esm|format-cjs <specifier or file> [from-folder]
// `shape-*` prints { names: { name: typeof value } } with CommonJS seen as an `import` of it would see it: with `__esModule` the exports
// are the names and `default` is the default export, without it `default` is module.exports and its own properties are the names. `format-*`
// prints esm or cjs for the file the specifier resolves to (so a package that ships one format for `import` and `require` shows it).
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const [mode, target, from = process.cwd()] = process.argv.slice(2);
const parent = join(resolve(from), 'noop.js');
const require = createRequire(parent);
const isFile = target.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(target) || target.startsWith('.');
const absolute = isFile ? resolve(from, target) : null;
const formatOf = (file) => {
  if (file.endsWith('.mjs') || file.endsWith('.mts')) return 'esm';
  if (file.endsWith('.cjs') || file.endsWith('.cts')) return 'cjs';
  for (let dir = dirname(file); ; dir = dirname(dir)) {
    const manifest = join(dir, 'package.json');
    if (existsSync(manifest)) {
      try { return JSON.parse(readFileSync(manifest, 'utf8')).type === 'module' ? 'esm' : 'cjs'; } catch { return 'cjs'; }
    }
    if (dir === dirname(dir)) return 'cjs';
  }
};
const describe = (value, namespace) => {
  const names = {};
  const kind = (v) => { try { return typeof v; } catch { return 'throws'; } };
  if (namespace) {
    for (const key of Object.keys(value)) names[key] = kind(value[key]);
    return names;
  }
  if (value && value.__esModule) {
    for (const key of Object.keys(value)) if (key !== '__esModule') names[key] = kind(value[key]);
    return names;
  }
  // Without `__esModule`, `default` is module.exports itself: a function is a default export worth matching, an object is the named exports.
  if (typeof value !== 'object') names.default = typeof value;
  if (value && (typeof value === 'object' || typeof value === 'function')) for (const key of Object.keys(value)) if (key !== 'default') names[key] = kind(value[key]);
  return names;
};
if (mode === 'format-esm') console.log(formatOf(fileURLOf(import.meta.resolve(isFile ? pathToFileURL(absolute).href : target, pathToFileURL(parent).href))));
else if (mode === 'format-cjs') console.log(formatOf(require.resolve(isFile ? absolute : target)));
else if (mode === 'shape-esm') {
  const spec = isFile ? pathToFileURL(absolute).href : import.meta.resolve(target, pathToFileURL(parent).href);
  console.log(JSON.stringify({ names: describe(await import(spec), true) }));
} else if (mode === 'shape-cjs') console.log(JSON.stringify({ names: describe(require(isFile ? absolute : target), false) }));
function fileURLOf(url) { return new URL(url).pathname.replace(/^\/([a-zA-Z]:)/, '$1'); }
