# rathkit-pack

A JavaScript and TypeScript bundler written in Rust with **no dependencies** (not even
crates), aimed at being faster, cleaner and safer than the incumbents, and at being easy to
move to.

It is early. This describes what exists today and how it is checked.

## What it does

```
rathkit-pack bundle src/main.ts --outfile dist/app.mjs
```

- Parses ES modules, CommonJS, JSON, TypeScript, JSX and TSX.
- Resolves like Node does (`exports`/`imports` maps with conditions, `node_modules`, `main`
  fields, `sideEffects`) plus the usual bundler extras: `tsconfig.json` `paths` and
  `baseUrl`, `.js` that means `.ts`, `--alias`.
- Links imports across modules (re-exports, `export *`, namespaces, cycles) and reports a
  missing or ambiguous name as an error before anything runs.
- Loaders (`--loader .ext=NAME`): `text`, `base64`, `dataurl`, `binary`, `file` (copied to the output as
  `[name]-[hash].[ext]`, see `--asset-names` and `--public-path`), `empty`, `css`, `local-css`. By default
  `.txt .md .html` are text, images, fonts, media, `.pdf` and `.wasm` are files, `.css` is a style sheet and
  `.module.css` has local class names (exported as a default object and as named exports; `composes` within a
  file or `from "./other.module.css"`). A sheet's `@import`s are inlined in order (`@import "x" screen` wraps what
  x holds), `url()` points at the copied files, and each chunk's sheets go to a `.css` file next to its `.js`
  (with `--sourcemap`, a `.css.map` too); `--minify` also shrinks the CSS. A copied file nothing uses is not written.
- Plugins (`--config FILE`, or `rathkit.config.{mjs,js,cjs}` or a `.parcelrc` next to the project): the config
  lists plugins and a Node process runs them, so nothing is needed when there is no config. All of these work
  together in one build:
  - **Rollup and Vite** plugins: `options`, `buildStart`, `resolveId`, `resolveDynamicImport`, `load`,
    `transform` (with source map chaining), `moduleParsed`, `buildEnd`, `renderChunk`, `generateBundle`,
    `writeBundle`, `closeBundle`, `watchChange`, `closeWatcher`, `resolveFileUrl`, `banner`/`footer`/`intro`/`outro`,
    hook filters, `enforce`, `apply`, and Vite's `config` and `configResolved`. Context: `resolve`, `load`,
    `emitFile` (assets, chunks and prebuilt chunks, with `import.meta.ROLLUP_FILE_URL_*`), `getFileName`,
    `getModuleInfo` / `getModuleIds` (import lists, importers, exports), `addWatchFile`, `meta.watchMode`,
    `parse` (the ESTree comes from this bundler's own parser, nothing needs installing), `warn`/`error`.
    `renderChunk` and `generateBundle` get real chunk information (modules, imports, dynamic imports, exports).
  - **esbuild** plugins (`setup(build)` with `onResolve`/`onLoad`/`onStart`/`onEnd`, namespaces, `pluginData`,
    `resolveDir`, loaders).
  - **webpack** loaders (`module.rules`: pitching, inline `!!loader!file` requests, `raw`, async, `this.getOptions`,
    `this.loadModule`/`importModule`, `type: asset/*`) **and plugins written against the compiler**: a real
    Tapable, `compiler` and `compilation` with the hooks webpack runs in its order (`environment` ... `make`,
    `finishModules`, `seal`, `processAssets` by stage, `emit`, `afterEmit`, `done`), assets (`emitAsset`,
    `updateAsset`, `renameAsset`, `deleteAsset`), `webpack-sources`, stats (`getStats().toJson()`), modules,
    chunks and entrypoints, normal module factory hooks, and child compilers (`createChildCompiler`,
    `runAsChild`). `DefinePlugin`, `EnvironmentPlugin`, `ProvidePlugin`, `BannerPlugin`, `IgnorePlugin`,
    `NormalModuleReplacementPlugin` and `EntryPlugin` are built in (so `webpack` need not be installed; `require('webpack')`
    falls back to this version); `MiniCssExtractPlugin` is done by the bundler's own style sheet pipeline.
    Real plugins tried: `copy-webpack-plugin`, `html-webpack-plugin`, `mini-css-extract-plugin` with
    `css-loader`, `compression-webpack-plugin`, `webpack-manifest-plugin`, `clean-webpack-plugin`.
  - **Parcel** plugins from `.parcelrc` (or `parcel` in the config): transformers, resolvers, reporters,
    optimizers, validators, compressors, namers and runtimes, with `@parcel/plugin` (the real one, or a small
    stand-in when it is not installed). Parcel's own plugins for JavaScript, CSS, HTML and JSON are what the
    bundler does itself. Real plugins tried: `@parcel/transformer-sass`, `-less`, `-yaml`, `-toml`, `-graphql`
    and `@parcel/compressor-gzip`.
  - Real Rollup plugins tried: `@rollup/plugin-json`, `-image`, `-commonjs`, `-node-resolve`, `-babel`,
    `-typescript`; webpack loaders: babel-loader, css-loader with style-loader.
  - Not supported: Vite's module-level HMR (`handleHotUpdate` can stop or add reloads, not swap modules), webpack plugins that
    depend on webpack's own module and chunk graph internals beyond the above (for example `SplitChunksPlugin`
    settings are ignored), Parcel packagers and bundlers (the bundler's own are used).
- `--watch` builds again when a file the build read changes (and plugins' `addWatchFile` files): plugins hear it
  through `watchChange`, a changed config starts everything again, and Ctrl-C ends it cleanly. Modules whose files
  did not change are kept between builds (no reading, parsing or scanning again; without plugins).
- `rathkit-pack serve ENTRY... [--port N] [--host H]` is a development server with the same options as `bundle`.
  The build stays in memory (with source maps) and is served over HTTP, together with your `index.html` (its entry
  `<script src>` is pointed at the built file; the style sheets and the reload client are added), `public/` and
  the project's files. A rebuild tells open pages: style sheet edits are swapped in place, anything else reloads
  the page, and build errors show as an overlay that goes away when the build is fixed. Vite plugins take part
  through `configureServer` (connect-style `server.middlewares`, `server.ws.send`, `server.watcher`),
  `transformIndexHtml` (strings, `{ html, tags }`, `order`) and `handleHotUpdate` (returning `[]` stops the
  reload; `server.ws.send` events reach `import.meta.hot.on`). `import.meta.hot` exists
  (`on`, `off`, `send`, `dispose`, `invalidate`, `data`; `accept` is accepted and unused). Replacing single
  modules in a running page is not possible: the output is one scope-hoisted program, so scripts reload.
- Tree-shakes at statement level, honoring `"sideEffects": false`.
- `--define KEY=VALUE` (also `--define:KEY=VALUE`) replaces a global name, a chain such as
  `process.env.NODE_ENV`, or `import.meta.env.X` with a constant, another name or an expression.
  Branches that follow (`if`, `?:`, `&&`, `||`, `??` on constants) are left out, with the modules only
  they `require()` or `import()`: React DOM's server renderer goes from 600 KB to 234 KB minified
  with `--define process.env.NODE_ENV='"production"'`. Assignments to a defined expression and
  `const { NODE_ENV } = process.env` are left alone, as in other bundlers. Imports used only in a
  dead branch are dropped with it.
- Emits one ES module with all modules' top-level code in a single scope (scope hoisting).
  Names are deconflicted; ES modules loaded by `import()` or `require()` start lazily;
  CommonJS modules run on first use.
- Output formats: `--format esm|cjs|iife` (`--global-name`, and `--global SPEC=Name` for
  externals in an IIFE), and `--splitting --outdir DIR` for ES modules, which splits shared
  code and `import()` targets into their own chunks.
- TypeScript is erased by editing the original text: types, `as`, `satisfies`, `!`,
  overloads, `abstract`, `declare`, `import type`. Enums, namespaces, parameter properties,
  `import x = require()` and `export =` become JavaScript. Decorators work in both kinds:
  standard (TC39) decorators and `accessor` members by default, and TypeScript's legacy
  decorators with `emitDecoratorMetadata` when `experimentalDecorators` is on, so NestJS-style
  dependency injection runs. A member of an enum whose value is a constant (`Color.Red`,
  `Color["Red"]`, also through imports and re-exports) is written as that value, and an enum
  nothing else reads is dropped. As TypeScript requires, members are taken to be read-only.
- `--minify` removes whitespace and comments, gives local bindings short names, scope by
  scope and most-used first, and writes the program shorter (`--minify-whitespace`,
  `--minify-identifiers` and `--minify-syntax` do one each; the last always writes compactly).
  The syntax part: `!0`/`!1`/`void 0` for `true`/`false`/`undefined`, the shortest spelling of
  numbers (`1e3`, `.5`), `a.b` for `a["b"]` and unquoted keys, `new X` for `new X()`, no braces
  around a single statement (never where an `else` would move), joined `var`/`let`/`const`,
  `() => x` for `() => { return x; }`, `return` for `return undefined`, and only the branch
  that runs of `if (true)`/`true ? a : b` (kept when the dropped code declares a `var`).
  A big bundle is cut between top-level statements and the pieces are parsed, renamed and
  written on all cores (only the top-level names are chosen for the bundle as a whole); the
  text is the same as one pass writes, except that declarations are not joined across a cut.
  A bundle that is one huge module is one piece. `RATHKIT_FRAGMENTS=off` turns the cutting off.
  Exported names, imported names, properties and globals never change, nor do bindings that
  a direct `eval` can see. `--keep-names` leaves functions, classes and variables that name an
  anonymous function alone, so their `.name` stays. It is on by itself when decorators are
  enabled (frameworks like Nest read class names at run time; `--no-keep-names` overrides).
- `--sourcemap` writes a version 3 source map beside each output file (`--sourcemap=inline` puts
  it in the file, `=external` leaves out the comment that points to it). It maps through
  TypeScript erasure, lowering, scope hoisting and `--minify`, back to the original files
  (TypeScript stays TypeScript), records the original name of every renamed identifier, and
  counts columns in UTF-16 units like engines do. A file that ships its own map (a trailing
  `sourceMappingURL` comment, inline or beside it) is traced on to what it was built from, and
  sources under `node_modules` are listed in `x_google_ignoreList`. The original text is embedded unless
  `--no-sources-content`. `node --enable-source-maps` and browser tools show original frames.
- JSX compiles to the automatic runtime (`react/jsx-runtime`) or the classic factory.

## Options

```
rathkit-pack bundle ENTRY... [--outfile FILE | --outdir DIR --splitting] [--no-treeshake]
    [--format esm|cjs|iife] [--global-name NAME] [--global SPEC=NAME]...
    [--minify | --minify-whitespace --minify-identifiers --minify-syntax] [--keep-names | --no-keep-names]
    [--sourcemap[=inline|external]] [--no-sources-content]
    [--config FILE | --no-config] [--mode NAME] [--watch]
rathkit-pack serve ENTRY... [--port N] [--host H] [the options above] [--define KEY=VALUE]... [--loader .ext=NAME]... [--asset-names T] [--public-path P] [--external NAME]... [--condition NAME]... [--main-field NAME]...
    [--alias NAME=PATH]... [--tsconfig FILE | --no-tsconfig]
    [--jsx automatic|classic] [--jsx-import-source S] [--jsx-factory F] [--jsx-fragment F]
    [--experimental-decorators] [--decorator-metadata] [--strict-null-checks] [--original-paths] [--platform-browser]
```

`tsconfig.json` is found automatically and supplies `paths`, `baseUrl`, `jsx`,
`jsxImportSource`, `jsxFactory`, `jsxFragmentFactory`, `experimentalDecorators`,
`emitDecoratorMetadata` and `strictNullChecks`, following `extends`.

Development commands (`--estree FILE`, which prints a file as ESTree JSON, `--globals FILE`, `--graph`, `--parse-check`, `--ts-verdicts`, `--resolve-dump`,
`--link-dump`, ...) are listed by running the binary with no arguments.

## How it is checked

Node, V8 and the TypeScript compiler are the oracles, and nothing here is trusted until it
agrees with them:

| Layer | Oracle | Scale |
|---|---|---|
| JavaScript parser | V8 | 20,000+ real files, plus fuzzed mutants |
| Scope analysis | V8 | 4,590 declaration-pair programs |
| Resolution | `require.resolve`, `import.meta.resolve` | 231,000 requests |
| Linking | Node's own module linker (link only) | 1,100 real closures, 32,000 random programs |
| TypeScript parser | `tsc`'s parser | 12,000 `.d.ts` files, 8,000 source files |
| Bundles | the same program run unbundled | 32 fixtures in nine output modes, 5 code-splitting programs, 6,000+ random multi-module programs, 570 real packages, real libraries doing real work (TypeScript, Prettier, Terser, Babel, React DOM) |
| Minifier | the same program under other names and spellings (same trees up to the syntax rewrites it is allowed, same literals, every identifier denotes the same binding), and the same output when run | 37,779 real modules; every bundle test above also runs minified; 1,000 random programs built from the constructs the rewrites touch (`tools/fuzz-syntax.mjs`) |
| Plugins | the same program with the plugins' effects, run by Node: virtual modules, aliases, transforms, hooks in order, emitted files and chunks, module info, `this.parse`, esbuild namespaces, webpack loaders with pitch and inline requests, webpack compiler hooks (lifecycle order, assets, stats, child compiler, built-in plugins), Parcel plugins of every kind, source maps through a transform (Node's own stack traces) | 10 programs x 3-4 output modes (`tools/run-plugins.mjs`); `--watch` (`tools/run-watch.mjs`); the ESTree of every `.js` file in a project's `node_modules` against acorn's (`tools/check-estree.mjs`; 223,000 files on the development machine); real Rollup, webpack and Parcel plugins tried by hand |
| Loaders, style sheets | the values a program reads from imported text, base64, data URLs, bytes and copied files; CSS text snapshots; every CSS source map segment leads back to the same token in the source | 3 programs x 7 output modes (`tools/run-loaders.mjs`); every style sheet in a project's `node_modules` maps correctly (76 on the development machine) |
| Defines | the same program run unbundled with the values put in place first, and no dead module left in the bundle | 3 programs x 7 variants x 6 output modes (`tools/run-defines.mjs`) |
| Minifying in pieces | the same bundle minified in one pass (`tools/check-pieces.mjs`: byte for byte, every format, many cuts and few) | every fixture and every package in a real project; source maps for a 1.1 MB NestJS bundle byte for byte |
| Source maps | the other end of every mapping (the token in the generated file is the token in the original, or the identifier the map says was renamed), and Node's own stack traces | 1.3 million mappings in the TypeScript compiler bundle, 20,000 error sites in generated TypeScript/JavaScript/CommonJS programs across 7 output modes, every fixture |
| JSX, decorators, metadata | `tsc`'s own transform | 18 programs in both JSX modes; the standard-decorator ones also as `cjs`, `iife` and unshaken |

```
cargo test                                    # unit tests and the Node comparisons
TS_MODULE=/path/to/typescript cargo test      # also the comparisons against tsc
node tools/run-fixtures.mjs                   # bundle fixtures vs Node
node tools/run-plugins.mjs                    # Rollup, Vite, esbuild, webpack and Parcel plugins through the host
node tools/run-watch.mjs                      # --watch: rebuilds, plugin hooks, restart on config change, Ctrl-C
node tools/run-serve.mjs                      # serve: pages, middlewares, html hooks, css swaps, reloads, errors
node tools/check-estree.mjs DIR...            # the ESTree of every .js file against acorn's (needs acorn; ACORN_DIR)
node tools/run-loaders.mjs                    # loaders and style sheets (UPDATE_SNAPSHOTS=1 rewrites the CSS snapshots)
node tools/run-defines.mjs                    # --define vs the values set in Node
node tools/run-sourcemaps.mjs                 # stack traces through source maps vs the original positions
node tools/check-pieces.mjs                    # minifying in pieces vs in one pass (add --packages PROJECT for every package)
node tools/check-sourcemap.mjs OUT.js         # one bundle's map against both ends
python3 tools/fuzz-bundle.py --seed 1         # random programs vs Node
node tools/diff-packages.mjs PROJECT          # every package in PROJECT/node_modules
node tools/diff-real.mjs PROJECT              # real library workloads
```

## Known behavior worth knowing

- Renaming a top-level function changes its `.name` (class names are preserved).
- A CommonJS module's namespace lists its enumerable keys at run time; Node derives names
  statically.
- A file is an ES module if it needs module syntax to parse, is `.mjs`, or lives in a
  `"type": "module"` package. CommonJS code ends up strict, because the output is an ES
  module.
- With a single output file, `import()` of a bundled module runs it on first use but does
  not load it from the network.
- A CommonJS entry written as `--format cjs` (or an IIFE with a global name) is its own `module.exports`, not an
  object with a `default`. A package with `"type": "commonjs"` may still use `import` in its `.js` files, as in
  other bundlers.
- `this.parse` shows a string with an unpaired surrogate (`"\ud800"`) as U+FFFD, because strings here are UTF-8.
- Emitted chunk files are written flat (no folders), and need `--format esm`.
- `new URL("./file.png", import.meta.url)` copies the file and points the string at the copy (a script there stays as written).
- `require.resolve("literal")` becomes the resolved path; `require(variable)` falls back to a
  `createRequire` rooted at the module's original location when `--original-paths` is set,
  and at the bundle otherwise.

The oracles that need outside packages (`acorn`, `typescript`, a project with a `node_modules`) are run by passing their
location (`ACORN_DIR`, `TS_MODULE`, a project path argument); without them `cargo test` skips those comparisons and says so.
The numbers above were measured on the development machine against its own `node_modules`.

## Requirements

Rust (stable) to build. Node.js **24 or later** is needed only when plugins are used (`--config`, `.parcelrc`, `serve` with
plugin hooks); plain builds never start it. An older Node is refused with a message that says so. The plugin suite,
`--watch` and `serve` are checked on Node 24.0 and 26.12. Plugins may import `webpack`, `tapable`, `webpack-sources` and
`@parcel/*` without installing them: stand-ins answer when the project has none.

## Zero dependencies

`Cargo.toml` has no `[dependencies]`, and `tools/check-zero-deps.mjs` fails the build if one
appears. Unicode tables are generated by `tools/gen-unicode-id.py`, JSON and glob matching are
written here, and the profiler is a small signal-based sampler.

## License

Apache License 2.0, see `LICENSE`.
