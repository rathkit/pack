# From Rollup

Rollup's own plugins for the usual work are what `rkp` does itself: `@rollup/plugin-node-resolve` (it resolves like Node, with
`exports` maps and conditions), `@rollup/plugin-commonjs` (CommonJS is read), `@rollup/plugin-replace` (`--define`), `@rollup/plugin-terser`
(`--minify`), `@rollup/plugin-json`, `@rollup/plugin-alias` (`--alias`) and `@rollup/plugin-typescript` (TypeScript is read). Your own plugins, and the
ones for what `rkp` does not do, stay: Rollup plugin hooks (`resolveId`, `load`, `transform`, `renderChunk`, `generateBundle`, `emitFile`
and the rest of the README's list) run from `rathkit.config.mjs`.

Before (`rollup.config.mjs`; Rollup 4.67.5 with the four plugins named; `rollup -c`):

```js
import path from 'node:path';
import { nodeResolve } from '@rollup/plugin-node-resolve';
import commonjs from '@rollup/plugin-commonjs';
import replace from '@rollup/plugin-replace';
import terser from '@rollup/plugin-terser';

const text = { name: 'text', transform: (code, id) => (id.endsWith('.txt') ? `export default ${JSON.stringify(code)};` : null) };
const at = { name: 'at', resolveId: (source) => (source.startsWith('@/') ? path.resolve('src', source.slice(2)) : null) };

export default {
  input: 'src/main.js',
  output: { file: 'dist/main.js', format: 'es', sourcemap: true },
  plugins: [
    at,
    text,
    nodeResolve(),
    commonjs(),
    replace({ preventAssignment: true, values: { __APP_VERSION__: '"1.2.3"', 'process.env.NODE_ENV': '"production"' } }),
    terser(),
  ],
};
```

After (`rkp` 0.3.0-beta.0). The config keeps the two plugins of the project's own (`rathkit.config.mjs` is found by name):

```js
import path from 'node:path';

// Only what rkp does not do itself: node-resolve, commonjs, replace and terser are built in.
const text = { name: 'text', transform: (code, id) => (id.endsWith('.txt') ? `export default ${JSON.stringify(code)};` : null) };
const at = { name: 'at', resolveId: (source) => (source.startsWith('@/') ? path.resolve('src', source.slice(2)) : null) };

export default { plugins: [at, text] };
```

and the options of Rollup's `output` and of the replaced values go on the command line:

```
rkp bundle src/main.js --outfile dist/main.js --define:__APP_VERSION__='"1.2.3"' --platform=browser --minify --sourcemap
```

Both programs print `hello world 42 aliased from a text file 1.2.3 production`. (The `rkp` file was 361 bytes and the Rollup and terser file 267
on this small program: not a benchmark, and not claimed to be smaller.)

## Options

| Rollup | `rkp` |
| --- | --- |
| `input` | the entries after `bundle` (several are fine) |
| `output.file`, `output.dir` | `--outfile FILE`, `--outdir DIR` |
| `output.format` `es`, `cjs`, `iife` | `--format esm`, `cjs`, `iife`; a list (`--format esm,cjs`) writes both |
| `output.format` `umd`, `amd`, `system` | not done: an error (see below) |
| `output.name` | `--global-name NAME` |
| `output.sourcemap` | `--sourcemap`, `--sourcemap=inline` |
| `output.entryFileNames`, `chunkFileNames`, `assetFileNames` | `--entry-names`, `--chunk-names`, `--asset-names` (`[name]`, `[hash]`, `[ext]`) |
| `output.banner`, `footer`, `intro`, `outro` | `--banner:js=`, `--footer:js=`; the hooks of a plugin |
| `output.preserveModules`, `preserveModulesRoot` | `--preserve-modules`, `--preserve-modules-root DIR` |
| `output.exports` | `--exports auto\|named` |
| `external` | `--external:NAME` (a name covers what is inside the package; one `*` is allowed) |
| `treeshake: false` | `--no-treeshake` |
| `watch` / `rollup -w` | `--watch` |
| `output.manualChunks` | not done (dynamic `import()` makes chunks with `--splitting`) |

## What is different

- **`umd`, `amd` and `system`.** `--format=umd` is an error. Use `iife` with `--global-name` for a page, or write both `esm` and `cjs`.
- **`@rollup/plugin-commonjs` in the config.** It does not run under `rkp` (the build stops with `[commonjs] Cannot read properties of
  null (reading 'meta')`), and it is not needed: take it out. `@rollup/plugin-node-resolve`, `-terser` and `-replace` load and run, but
  are redundant (replace with `--define` to avoid one more dependency).
- **Library mode.** `--format esm,cjs`, `--write-exports` (sets `main`, `module`, `types` and the `exports` map in `package.json`) and
  `--dts` for declaration files replace the usual `rollup-plugin-dts` and a hand-written `exports` map.

`rkp migrate` reads a config for the plain strings in its text. When the input or the output is not one (a list built by a function, a path built with `path.join`), it runs the config in Node's permission model (reads only: no file writes, no child processes; `--no-run` turns that off) and prints one command for each output of the list, as `build:rkp`, `build:rkp-2`, and so on. A `umd` output becomes `--format=iife --global-name=NAME`, and the note says so.
