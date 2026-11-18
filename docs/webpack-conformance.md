# webpack conformance

What of webpack's configuration, and of the loaders and plugins people use most with it, works when `rkp bundle` is given a
`webpack.config.js` (or `--config FILE`). Every row marked **works** has a fixture under
[`tests/webpack-conformance/`](../tests/webpack-conformance/) named in the last column. `node tools/run-webpack-conformance.mjs`
builds each one with the real package from `test-deps/` and runs the output with Node (install with
`pnpm install --dir test-deps --frozen-lockfile --ignore-scripts`; without it the suite is skipped).

Status: **works** (fixture passes), **partly** (the fixture covers what is said), **no** (not supported; see
the gaps at the end), **n/a** (has no meaning for this bundler).
The fixtures target Node (`target: 'node'`) where they print their result; that is how they are checked, not a limit.

Pack is one scope-hoisted bundler, not webpack: there is no module table and no webpack runtime, so settings that describe
webpack's runtime or its caches do nothing.

## Configuration options

| Option | Status | Notes | Fixture |
|---|---|---|---|
| `entry` (string, array, object, `{ import }`) | works | names are not kept; all entries are built. A function is not read | `cfg-entry` |
| `mode` | works | sets `process.env.NODE_ENV` (`optimization.nodeEnv` overrides) | `cfg-mode-development`, `cfg-mode-production` |
| `output.path`, `output.filename` | partly | the command line (`--outdir`, `--entry-names`) decides; a `/` in a name is not supported | gap 1 |
| `output.publicPath` | works | used by asset modules and file URLs | `cfg-output-publicpath` |
| `output.assetModuleFilename` | works | `[name] [base] [ext] [path] [hash] [contenthash] [query]` | `asset-resource` |
| `output.library`, `output.libraryTarget` | no | gap 2 | |
| `output.clean` | n/a | the plugins' own cleaning is covered (`clean-webpack-plugin`) | |
| `module.rules` (`test`, `include`, `exclude`, `resourceQuery`, `resource`, `use`, `loader`, `options`, `enforce`) | works | | every loader fixture |
| `module.rules` `oneOf`, nested `rules` | works | | `cfg-rules-oneof-issuer` |
| `module.rules` `type: asset`, `asset/resource`, `asset/inline`, `asset/source`; `parser.dataUrlCondition`; `generator.filename` / `publicPath` / `dataUrl` | works | | `asset-*` |
| `module.rules` `issuer`, `issuerLayer`, `dependency`, `mimetype`, `scheme` | no | not read | gap 3 |
| `module.noParse` | works | | `cfg-noparse` |
| `resolve.alias` (including `name$`) | works | | `cfg-resolve-alias` |
| `resolve.extensions` (with `...`) | works | | `cfg-resolve-extensions` |
| `resolve.modules` (absolute directories) | works | relative names mean `node_modules` as usual | `cfg-resolve-modules` |
| `resolve.mainFields` | works | | `cfg-resolve-mainfields` |
| `resolve.conditionNames` | works | `import`/`require`/`default` are chosen by the request | `cfg-resolve-conditionnames` |
| `resolve.extensionAlias` | works | `.js` meaning `.ts` also works with no config | `cfg-resolve-extensionalias` |
| `resolve.fallback`, `resolve.alias` `false` | works | `false` gives an empty module | `cfg-resolve-fallback` |
| `resolve.plugins`, `resolve.symlinks`, `resolve.roots`, `resolve.exportsFields` | no | not read | gap 3 |
| `resolveLoader` | partly | loaders are resolved from the project like Node does | |
| `externals` (string, RegExp, object, function, callback form, promise, array) | works | the module type in `commonjs x` picks the import; a global (`var X`) is imported by its name, not read from `window` | `cfg-externals`, `cfg-externals-function`, `webpack-node-externals` |
| `externalsType`, `externalsPresets` | no | gap 3 | |
| `devtool` | works | `source-map`, `inline-*`, `hidden-*`, `nosources-*`; the `eval` kinds make none | `cfg-devtool` |
| `target` | partly | `web` and `node` as the bundler's own platform; no `electron-*`, `webworker`, browserslist query | `cfg-target-web` |
| `node` (`__dirname`, `__filename`, `global`) | partly | | `cfg-node-dirname` |
| `optimization.minimize`, `minimizer` (with `'...'`) | works | the plugins listed run; `'...'` is the bundler's own `--minify` | `terser-webpack-plugin`, `css-minimizer-webpack-plugin` |
| `optimization.splitChunks` | works (the `cacheGroups` you list) | the cache groups decide the layout; the size-splitting settings and webpack's implicit groups are n/a; see the SplitChunksPlugin section | `cfg-splitchunks*` |
| `optimization.runtimeChunk`, `moduleIds`, `chunkIds`, `usedExports`, `sideEffects`, `concatenateModules`, `innerGraph` | n/a | tree shaking and scope hoisting are always on | |
| `experiments` (`topLevelAwait`, `outputModule`) | works | top-level await needs the esm format | `cfg-experiments` |
| `experiments` (`asyncWebAssembly`, `layers`, `css`, `lazyCompilation`) | no | gap 3 | |
| `plugins` | works | see below | |
| `stats`, `performance`, `infrastructureLogging`, `cache`, `snapshot`, `watchOptions`, `devServer`, `recordsPath`, `parallelism` | n/a | accepted, unused (`rkp serve` and `--watch` are the replacements) | |
| `context`, `name`, `dependencies`, `ignoreWarnings`, `bail` | partly | `context` is read for `entry` | `cfg-entry` |
| a function or an array of configs | partly | an array builds its first config | |
| `webpack-merge` | works | | `webpack-merge` |

## Plugins that ship with webpack

| Plugin | Status | Fixture |
|---|---|---|
| `DefinePlugin` (nested objects, `typeof x`) | works | `define-plugin` |
| `EnvironmentPlugin` | works | `environment-plugin` |
| `ProvidePlugin` (a module, a member of one) | works | `provide-plugin` |
| `BannerPlugin` | works | `banner-plugin` |
| `IgnorePlugin` | works | `ignore-plugin` |
| `NormalModuleReplacementPlugin` | works | `normal-module-replacement` |

## The 30 loaders and plugins most used

Chosen from what the package registry downloads the most with webpack, as far as I know it; the list is a judgment, and a
package not listed may work.

| # | Package | Status | What the fixture does | Fixture |
|---|---|---|---|---|
| 1 | `css-loader` | works | CSS text through the loader | `css-loader` |
| 2 | `style-loader` | works | inserts a `<style>` tag (a stand-in `document`) | `style-loader` |
| 3 | `babel-loader` | works | `@babel/preset-env` to ES5 | `babel-loader` |
| 4 | `ts-loader` | works | `transpileOnly` | `ts-loader` |
| 5 | `swc-loader` | works | TypeScript | `swc-loader` |
| 6 | `esbuild-loader` | works | TypeScript | `esbuild-loader` |
| 7 | `sass-loader` | works | nesting and variables | `sass-loader` |
| 8 | `less-loader` | works | nesting and variables | `less-loader` |
| 9 | `postcss-loader` | works | autoprefixer through `postcssOptions` | `postcss-loader` |
| 10 | `file-loader` | works | the file is written | `file-loader` |
| 11 | `url-loader` | works | a data URL under the limit | `url-loader` |
| 12 | `raw-loader` | works | | `raw-loader` |
| 13 | `html-loader` | works | `<img src>` becomes an asset | `html-loader` |
| 14 | asset modules (`asset/*`) | works | four types | `asset-resource`, `asset-inline`, `asset-source`, `asset-auto` |
| 15 | `html-webpack-plugin` | works | title, meta, script tag | `html-webpack-plugin` |
| 16 | `mini-css-extract-plugin` | works | the sheet is a file of its own (written by the bundler's pipeline) | `mini-css-extract-plugin` |
| 17 | `copy-webpack-plugin` | works | a folder and a renamed file | `copy-webpack-plugin` |
| 18 | `terser-webpack-plugin` | works | minifies the chunk | `terser-webpack-plugin` |
| 19 | `css-minimizer-webpack-plugin` | works | | `css-minimizer-webpack-plugin` |
| 20 | `clean-webpack-plugin` | works | | `clean-webpack-plugin` |
| 21 | `compression-webpack-plugin` | works | `.gz` written | `compression-webpack-plugin` |
| 22 | `webpack-manifest-plugin` | works | | `webpack-manifest-plugin` |
| 23 | `webpack-bundle-analyzer` | works | static report | `webpack-bundle-analyzer` |
| 24 | `dotenv-webpack` | works | | `dotenv-webpack` |
| 25 | `fork-ts-checker-webpack-plugin` | works | `async: false` | `fork-ts-checker-webpack-plugin` |
| 26 | `eslint-webpack-plugin` | works | an error fails the build (checked by hand, not a fixture) | `eslint-webpack-plugin` |
| 27 | `webpack-node-externals` | works | | `webpack-node-externals` |
| 28 | `workbox-webpack-plugin` | works | `GenerateSW` | `workbox-webpack-plugin` |
| 29 | `thread-loader` | works | passes through (loaders run in the host) | `thread-loader` |
| 30 | `worker-loader` | no | gap 4 | |

More that have a fixture: `source-map-loader`, `expose-loader`, `imports-loader`, `exports-loader`, `null-loader`, `yaml-loader`,
`json5-loader`, `circular-dependency-plugin`, `case-sensitive-paths-webpack-plugin`, `assets-webpack-plugin`,
`webpack-subresource-integrity`, `webpack-merge`. `node-polyfill-webpack-plugin` has a fixture that shows only that the plugin loads and the build runs: its
polyfills are not applied (gap 7).

## SplitChunksPlugin settings

`optimization.splitChunks.cacheGroups` that the config lists decide where modules go. Everything below needs `--splitting` (the esm output format), as
chunks do; without it the build is one file and a warning says so.

Only groups the config writes are applied. webpack's own `defaultVendors` and `default` groups (what `splitChunks: { chunks: 'all' }` implies in
webpack) are not added: they would change the output of every build with a dynamic `import()`. A config with `splitChunks` and no `cacheGroups` makes
the chunks Pack always makes (`cfg-splitchunks`); a program built either way prints the same.

What a group does, exactly:

- `test` (a RegExp, a string, a function of the module, or none for every module) runs in the plugin host, for all modules in one call after the graph is built.
  A RegExp or a string is tested against the module's file path (`module.nameForCondition()`); a string matches the start of the path. Paths are given to
  `test` with forward slashes, on every system, so `/[\\/]node_modules[\\/]/` works and a pattern that spells only `\\` does not on Windows.
  A function gets the module (`resource`, `nameForCondition()`, `type`, `size()`) and `{ chunkGraph, moduleGraph }` (webpack's real ones are not there: they
  are empty objects). A `test` that throws matches nothing, with a warning. A style sheet or an asset is never in a group.
- A module is reached from roots: the entries, and the modules an `import()` loads. `chunks: 'initial'` matches modules an entry reaches without an
  `import()` on the way, `'async'` (the default, as in webpack) modules only `import()` reaches, `'all'` both. A function `chunks` is not supported: the group is left out, with a warning.
- `minChunks` (default 1) is the number of roots, of the kinds `chunks` allows, that reach the module.
- `minSize` is the total text size of the modules the group takes, in bytes (webpack's default, 20000 in production and 10000 otherwise, when not given; a
  number, or `{ javascript }`). A group below it is not applied, and its modules go to the next group that matches or stay where they were. For a group
  without a `name` the size is that of one chunk. `enforce: true` ignores `minSize` and `minChunks`.
- `priority` (default 0): of the groups that match a module, the highest wins; on a tie, the one listed first.
- `name`, a string: the group makes one chunk, `<name>.js`. Without a `name` (or with a function, which is not followed) the group makes one chunk for each set of
  roots that need the modules, so it never joins modules different roots need: `<key>-<hash>.js`.
- A root is never moved out of its own chunk: an entry, or a module an `import()` loads, stays in its file even when a group's `test` matches it.
- A group that would make chunks that import each other (a `vendors` chunk whose package calls a module of the app's own chunk, which imports the package) is
  given up, with a warning that names it; its modules stay where they would be without it. The output never has a chunk cycle.

| Setting | Status | Fixture |
|---|---|---|
| `cacheGroups[key].test` (RegExp, string, function, none) | works | `cfg-splitchunks-groups`, `cfg-splitchunks-cachegroups` (a CommonJS package in `node_modules`), `cfg-splitchunks-functions` |
| `name` (a string) | works | `cfg-splitchunks-groups` |
| `chunks` (`'initial'`, `'async'`, `'all'`) | works | `cfg-splitchunks-chunks` |
| `priority` | works | `cfg-splitchunks-priority` |
| `minChunks` | works | `cfg-splitchunks-minchunks` |
| `minSize` (and `enforce`) | works | `cfg-splitchunks-minsize` |
| group cycles | works: the group is given up with a warning | `cfg-splitchunks-cycle` |
| `cacheGroups[key]: false` (and `defaultVendors: false`) | works: no group | `cfg-splitchunks-cachegroups` |
| `chunks` or `name` as a function | not supported: a `chunks` function leaves the group out, a `name` function makes the group unnamed | `cfg-splitchunks-functions` |
| `filename`, `reuseExistingChunk`, `automaticNameDelimiter`, `usedExports`, `hidePathInfo`, `maxAsyncRequests`, `maxInitialRequests` | accepted, not followed (the chunk file is named by `name`; a chunk has no request limit) | `cfg-splitchunks-cachegroups` |
| `maxSize`, `maxAsyncSize`, `maxInitialSize`, `minSizeReduction`, `enforceSizeThreshold` | n/a: they cut a chunk into pieces by size, so that each request loads a small file; Pack keeps a chunk whole, and code of one module is never split across files | `cfg-splitchunks-sizes` |
| `optimization.runtimeChunk` | n/a: there is no webpack runtime (the module loader and chunk table) to put in a chunk of its own; esm chunks import each other | `cfg-splitchunks-sizes` |
| `optimization.moduleIds`, `chunkIds` | n/a: there are no module or chunk ids in the output (modules are bindings in a scope, chunks are files named by `name` or by a hash) | `cfg-splitchunks-sizes` |

Not checked on Windows (no runner for it here): the path form `test` sees (forward slashes, from the path the resolver gives) and that a module's path
matches the one the group was tested with are written to be the same string on every system, but only Linux runs the fixtures; the Windows CI legs are the first run there.

## Plugins that read the compilation

Plugins get a `compiler` and a `compilation` with the parts they usually read. `compilation-internals` is a plugin written for the fixture that reads
`chunks`, `chunkGroups`, `entrypoints` (and their files), `modules`, `chunkGraph.getChunkModules`, `namedChunks`, `getAssets()`, `hash`, and
`stats.toJson()` (`assets`, `chunks`, `entrypoints`, `modules`), emits an asset in `processAssets`, and writes what it saw; its `check.mjs` fails if any
of it is missing. The real plugins that read the compilation are the rows of the table above: `html-webpack-plugin` (entrypoints, chunk files, asset
tags), `webpack-manifest-plugin`, `assets-webpack-plugin`, `webpack-bundle-analyzer` (stats), `workbox-webpack-plugin`, `compression-webpack-plugin`,
`circular-dependency-plugin` (module graph), `webpack-subresource-integrity` (`RealContentHashPlugin` hooks, chunk group iterables),
`fork-ts-checker-webpack-plugin`, `eslint-webpack-plugin`, `mini-css-extract-plugin` and `copy-webpack-plugin`. None of them needed an exception.

## Gaps

Each of these is a candidate for an issue:
1. `output.filename` / `chunkFilename` templates with folders or `[contenthash]` are not followed.
2. `output.library` and `libraryTarget` (UMD, var, assign) are not read.
3. Rule conditions `issuer`, `issuerLayer`, `dependency`, `mimetype`; `resolve.plugins`, `symlinks`, `roots`; `externalsType`; experiments `css`, `layers`, `asyncWebAssembly`.
4. `worker-loader` needs webpack's child compilation of a worker entry with the web worker template plugins.
5. Externals with a global (`{ react: 'React' }`, `var X`) are imported by name instead of being read from the global.
6. `optimization.splitChunks`: webpack's implicit `defaultVendors` and `default` groups are not applied (only the groups a config lists are); `chunks` and `name` as functions, `filename`, `reuseExistingChunk` and the request limits are not followed (see the SplitChunksPlugin section).
7. `node-polyfill-webpack-plugin` loads, but its polyfills are not applied: in a browser build (`--platform-browser`) `import 'path'` is "Cannot find 'path'", because the `resolve.fallback` entries the plugin adds for Node's built-in modules are not read. On a Node build the built-in module is used, which is why the fixture passes.
