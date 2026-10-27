# Rathkit Pack

A JavaScript and TypeScript bundler written in Rust, with zero dependencies (not even crates).

> Fast, clean and safe, and easy to move to: plugins written for other bundlers run (what is covered by tests is listed below).

**Status: alpha.** It is early: options, output and plugin support can change between 0.x releases (see [CHANGELOG.md](CHANGELOG.md), which also says what the version numbers promise). This describes what exists today and how it is checked. How to check a release: [docs/verifying.md](docs/verifying.md).

Site: <https://rathkit.com>

## Speed

Measured on seven workloads in three modes (21 cases: the TypeScript compiler, three.js, lodash-es, React, rxjs, two
generated apps; plain, minified, minified with a source map), the whole command took the least time and the least
memory in every case, against esbuild, rolldown, rspack and Vite. The closest was the TypeScript compiler minified
(397 ms against esbuild's 424 ms). Output sizes are about equal, and others are smaller in some cases. The method,
the machine, the versions and every table: [docs/benchmarks.md](docs/benchmarks.md). One machine, one day: run
`bench/run.mjs` for yours.

## Install

```
pnpm add -D @rathkit/pack@next        # the alpha is on the next tag
```

The package gives the `rkp` command and a JavaScript API (`import { build } from '@rathkit/pack'`, with the command's flags as options: [npm/pack/README.md](npm/pack/README.md)). Or take a tarball from [the releases](https://github.com/rathkit/pack/releases) (checksums and signatures: [docs/verifying.md](docs/verifying.md)), or `cargo build --release` from source.

## What it does

```
rkp bundle src/main.ts --outfile dist/app.mjs
```

(`rkp` is the command. The project is Rathkit Pack; the crate is `rathkit-pack` and the npm package is `@rathkit/pack`.)

- Parses ES modules, CommonJS, JSON, TypeScript, JSX and TSX.
- Resolves like Node does (`exports`/`imports` maps with conditions, `node_modules`, `main`
  fields, `sideEffects`) plus the usual bundler extras: `tsconfig.json` `paths` and
  `baseUrl`, `.js` that means `.ts`, `--alias`.
- Options in the style of esbuild's: `--banner:js=TEXT` / `--banner:css=TEXT` (`--banner=TEXT` is for scripts) and the same for
  `--footer`, put around every file of that kind (source maps are moved to match; a banner goes before a `#!` line too, so a
  script that needs one should put it in the banner); `--drop:console` (every `console.*(...)` call of the global `console`
  becomes `(void 0)`, arguments included) and `--drop:debugger`; `--packages=external` (every package import is left out of the
  bundle: a name that is not a path, and not made into one by an alias); `--resolve-extensions=.ts,.js`; `--jsx-dev` (JSX to `jsxDEV` of `<import source>/jsx-dev-runtime`, with the file, line and column of each element, as React's development
  builds want; `"jsx": "react-jsxdev"` in a tsconfig does the same); `--tsconfig-raw=JSON` (a tsconfig given as text, used in place of any
  file); `--inject:FILE` (what the file exports stands in for the globals of those names, in every module that uses one and does not
  declare it; a file no module needs is not bundled); `--legal-comments=eof|none|linked|external` (the comments that say who wrote code and under what terms: `/*! ... */`, `@license`,
  `@preserve`; minifying drops comments, so these are put after the code of the file that has them, each once (`eof`, the default), left
  out, or put in `NAME.LEGAL.txt`, named at the end of the file or not); `--entry-names=[name]-[hash]` and `--chunk-names=[name]-[hash]` (templates for the names of the files of entries and of the rest; `[hash]`
  is a hash of what is in the file and in the files it loads, so a name changes when anything the file runs does, which is what
  caching by name needs; no directories in a name) and `--out-extension:.js=.mjs` (or `.css=`);
  `--pure:NAME` (calls of a name, or of a chain such as `styled.div`, and tagged templates of it, are free of effects, so an unused one
  is removed when what it is given has no effects either); `--preserve-symlinks`; `--metafile=FILE` (JSON in the shape of esbuild's metafile: every
  module with what it imports, every file written with the modules in it, what it imports and exports and its entry point;
  `bytesInOutput` is an estimate, the modules' original sizes scaled to the file's size).
  A word that starts with a dash and is not an option is an error, not a file.
- Pages: `rkp bundle index.html --outdir dist` builds the module scripts the page points at, writes the page again with
  the built files (and their style sheets) in place, runs plugins' `transformIndexHtml` with the built files as `bundle`
  and the entry chunk, and copies `public/`. `<script type="module" src>` tags
  and `<link rel="stylesheet" href>` tags that point at files of the project are built, and the page points at the built files; links to other sites are left as written. The images, icons and media a page names (`<img src>`, `<link rel="icon">`, `<video poster>`, `url()` in `<style>`) are copied under a name with a hash; a file in `public/` stays where it is, and one that is nowhere stays as written (`srcset` and `style=""` attributes are not read). With ES modules (the default) a page's scripts and sheets are separate files and scripts that share code share a file. With `--format iife` everything is one file that the page loads once (the other script and sheet tags are removed). `--format cjs` is an error: a browser cannot load CommonJS.
- Imports of a path with a variable, `import(\`./locales/${name}.js\`)` or `import('./locales/' + name + '.js')`, as
  `@rollup/plugin-dynamic-import-vars` reads them: the path starts with `./` or `../`, a variable stands for one name (never
  crosses a `/`), and the extension is written out after the last variable. Each file that fits becomes an `import()` that the
  chosen path picks among (an unknown path rejects with "Unknown variable dynamic import"), so code splitting makes a file of
  each. Anything else is left as written, with a warning when it looks like it was meant to work; `import(name)` alone and
  package names are left quietly. A file added to a folder starts a build again when watching.
- `import.meta.env`, as Vite makes it: `MODE` (`production` for a build, `development` for `serve`, or `--mode`), `DEV`, `PROD`
  (`--mode development` or `production` decides; any other mode follows the command), `SSR` (false) and `BASE_URL` (`/`, or
  `--public-path`), and the variables of `.env`, `.env.local`, `.env.[mode]` and `.env.[mode].local` in the project folder (or
  `--env-dir`) whose names start with `VITE_` (or `--env-prefix`, which can be given again); the rest of a `.env` file is not put
  in the output. A later file wins, the real environment wins over the files, `--define:import.meta.env.NAME=...` wins over both,
  and `$NAME`, `${NAME}` and `${NAME:-default}` in a value are replaced once the files are merged (not in single quotes).
  `import.meta.env` alone is an object of all of these. `--no-env-files` reads no files; a change to a `.env` file starts a
  build again when watching.
- `--target=es2015` ... `es2025` (or `esnext`, the default, which rewrites nothing): the version of JavaScript the output may use.
  Syntax newer than it is rewritten, as `--target` does in esbuild: `a ** b` and `**=` (`Math.pow`), `?.`, `??`, `||=` `&&=` `??=`
  (a value used twice goes in a temporary `var` declared once at the top of the file), numeric separators, `catch` without a
  binding, a trailing comma in a call or parameter list, and object spread (`__spreadValues` and `__spreadProps`, which keep getters
  and methods) and object rest (`__objRest`) in a declaration, a function parameter (also of an arrow with an expression body)
  or a `for of`, and class fields, private members (`#x`, private methods and accessors, `#x in o`) and static blocks
  (`__publicField`, and a `WeakMap` or `WeakSet` per private name, as esbuild does), and async functions, methods and arrows (generators run by `__async`, each `await` a `yield`), `for await` loops and async generators (`__forAwait`, `__asyncGenerator`, and `yield*` through `__yieldStar`), and `super` inside an async method or arrow (read and written through accessors made where the `super` is valid). `es2015` is the lowest target. Anything the target cannot have and this
  cannot rewrite stops the build with a message that names it and the target, never passes through: BigInt literals can never be rewritten, and top-level await is rewritten only when the program is one file with nothing exported (an app: the whole program becomes the body of an async function, or of a generator run by `__async` below es2017, with the imports left outside), otherwise it stops the build. Not rewritten either: decorators together with
  class fields, `super` in a private method or a static initializer, a tagged template on a private member, or object rest anywhere else (nested in another pattern, in an assignment, in `catch`), or a `__proto__`
  property after a spread. Regular expression syntax and library functions are not looked at, and engine targets such
  as `chrome80` are not supported. Checked by running programs as written and rewritten for every target in plain, minified,
  CommonJS and IIFE form, and by parsing each result as the language of its target (`tools/run-target.mjs`).
- `--dts` (and `--dts-dir=DIR`, for a folder other than the output's): declaration files for the entries, written with the project's own
  `typescript` package (run by Node; it is an error if it is not installed). They are what `tsc --declaration --emitDeclarationOnly`
  writes: one `.d.ts` for each source file the entries reach, laid out from the folder the sources share (so `src/index.ts` becomes
  `dist/index.d.ts` beside `dist/index.js`), not merged into one file. A JavaScript entry gets its JSDoc types. Type errors are listed and do
  not stop the files being written. In the JS API: `dts: true`, `dtsDir`. Checked by type-checking a consumer against the result
  (`tools/run-dts.mjs`).
- Vite's import forms: `./a.wasm?init` (a function that instantiates the module and resolves to the instance, reading the copied file with
  Node's `fs` in Node and with `fetch` in a browser; a plain `./a.wasm` is the file's address, and ES module integration of
  WebAssembly (`import { add } from './a.wasm'`) is not supported), `./x.svg?raw` (text), `?url` (the copied file's address), `?inline` (a data URL), any other
  query stays on the module name for plugins (`?react`); `import.meta.glob("./dir/*.js", { eager, import, query, base })`
  (literal arguments; `*`, `**`, `?`, `[a-z]`, `{a,b}`, `!` to leave files out; a file added to a listed directory
  starts a new build when watching); workers: `new Worker(new URL("./w.js", import.meta.url))`, `./w.js?worker`,
  `?sharedworker`, `?worker&inline` and `?worker&url`. A worker script is built on its own as an ES module (shared
  code is repeated in it) and copied beside the output; workers need an ES module environment.
- Links imports across modules (re-exports, `export *`, namespaces, cycles) and reports a
  missing or ambiguous name as an error before anything runs.
- Loaders (`--loader .ext=NAME`): `text`, `base64`, `dataurl`, `binary`, `file` (copied to the output as
  `[name]-[hash].[ext]`; `[ext]` has no dot, and the extension is added when the name has no `[ext]`; see `--asset-names` and `--public-path`), `empty`, `css`, `local-css`. By default
  `.txt .md .html` are text, images, fonts, media, `.pdf` and `.wasm` are files, `.css` is a style sheet and
  `.module.css` has local class names (exported as a default object and as named exports; `composes` within a
  file or `from "./other.module.css"`). In a style sheet `url(/fonts/a.woff2)` is a file of the project (its folder, then `public/`), and stays as written when it is nowhere. A sheet's `@import`s are inlined in order (`@import "x" screen` wraps what
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
  - **Parcel** plugins from `.parcelrc` (or `parcel` in the config): transformers, resolvers, reporters,
    optimizers, validators, compressors, namers and runtimes, with `@parcel/plugin` (the real one, or a small
    stand-in when it is not installed). Parcel's own plugins for JavaScript, CSS, HTML and JSON are what the
    bundler does itself.
  - **What is covered by tests.** Plugin support is claimed only as far as this list goes.
    - Checked in CI on every change: the plugin hooks and contexts above, with small plugins written for the
      purpose (`tools/run-plugins.mjs`, 10 programs); and these real packages, run from `bench/`:
      `@rollup/plugin-url`, `@svgr/rollup`, `vite-plugin-svgr`, `rollup-plugin-copy`, webpack's `url-loader`,
      `file-loader` and `@svgr/webpack`, `vite-imagetools`, `vite-plugin-image-optimizer`, `unplugin-fonts` (its style sheet, and the tags it adds to a page in a build and in `serve`), and `vite-plugin-webfont-dl` (downloads from Google Fonts, so it
      is skipped when the network is down) (`tools/run-assets.mjs`), and `@module-federation/vite` and
      `@module-federation/enhanced` (`tools/run-federation.mjs`, in Chromium).
    - Tried by hand, not repeated in CI: `@rollup/plugin-json`, `-image`, `-commonjs`, `-node-resolve`, `-babel`,
      `-typescript`; babel-loader; css-loader with style-loader; `copy-webpack-plugin`, `html-webpack-plugin`,
      `mini-css-extract-plugin`, `compression-webpack-plugin`, `webpack-manifest-plugin`,
      `clean-webpack-plugin`; `@parcel/transformer-sass`, `-less`, `-yaml`, `-toml`, `-graphql` and
      `@parcel/compressor-gzip`.
    - Everything else a plugin might do is untested: a plugin not listed may work, and a bug report with
      the plugin's name is welcome.
  - Not supported: Vite's module-level HMR (`handleHotUpdate` can stop or add reloads, not swap modules), webpack plugins that
    depend on webpack's own module and chunk graph internals beyond the above (for example `SplitChunksPlugin`
    settings are ignored), Parcel packagers and bundlers (the bundler's own are used).
- `--watch` builds again when a file the build read changes (and plugins' `addWatchFile` files): plugins hear it
  through `watchChange`, a changed config starts everything again, and Ctrl-C ends it cleanly. Modules whose files
  did not change are kept between builds (no reading, parsing or scanning again; without plugins).
- `rkp serve ENTRY... [--port N] [--host H]` is a development server with the same options as `bundle`.
  The build stays in memory (with source maps) and is served over HTTP, together with your `index.html` (its entry
  `<script src>` is pointed at the built file; the style sheets and the reload client are added), `public/` and
  the project's files. A rebuild tells open pages: style sheet edits are swapped in place, anything else reloads
  the page, and build errors show as an overlay that goes away when the build is fixed. Vite plugins take part
  through `configureServer` (connect-style `server.middlewares`, `server.ws.send`, `server.watcher`),
  `transformIndexHtml` (strings, `{ html, tags }`, `order`) and `handleHotUpdate` (returning `[]` stops the
  reload; `server.ws.send` events reach `import.meta.hot.on`). `import.meta.hot` exists
  (`on`, `off`, `send`, `dispose`, `invalidate`, `data`; `accept` is accepted and unused). Replacing single
  modules in a running page is not possible in this mode: the output is one scope-hoisted program, so scripts reload.
  With `--hmr` (`rkp serve ... --hmr`; `hmr: true` in the JS API) the modules are a table of functions instead
  (nothing is renamed, shaken or minified), and a rebuild sends the page only the modules whose text changed:
  `import.meta.hot.accept()` (a module accepts its own new version), `accept("./dep", (mod) => ...)` and
  `accept(["./a", "./b"], ([a, b]) => ...)` (an importer accepts the new version of what it imports), `dispose(data => ...)`,
  `data` (kept between versions), `decline()` and `invalidate()` work as in Vite. A change walks up the importers to the
  modules that accept it, those run again, and the page reloads when it reaches an entry that nothing accepts or when a file
  that is not a module changes; style sheets are swapped as before. What an importer reads from a replaced ES module is
  the new bindings (its exports object is the same one). React Fast Refresh is built in, and on when the project has `react` and
  `react-refresh` installed (`--react-refresh` asks for it and says so if the package is missing, `--no-react-refresh` turns it off): modules
  of the project that use React register their components and the hooks of their functions as Babel's `react-refresh` plugin does,
  the page gets the runtime of the project's own package before its scripts, and a module whose exports are all components is
  replaced in place with the state of its components kept (a change to the hooks of a component, or `// @refresh reset`, starts its state again). A module that
  also exports something that is not a component is replaced with the modules that import it. Top-level await works (a module that awaits, and the ones that import it, run as async functions, and an update
  of one is applied when it has run), as do decorators, enums and namespaces. `--hmr` does not work with module federation (it
  says so), and keeps CommonJS packages as they are, so a package's own code is not replaced when it changes.
- Plugins in the JS API: `build({ plugins: [...] })` and `serve({ plugins: [...] })` take plugin objects (Rollup's hooks,
  and Vite's `config`, `configResolved`, `transformIndexHtml`, `handleHotUpdate`), which stay in your process: the plugin host
  calls their hooks across a local socket, so what goes in and out is JSON (bytes as `Buffer`), a plugin keeps its own state as
  usual, and `generateBundle` / `writeBundle` can change the `code` and `source` of the files or remove them. A hook's `this` has
  `resolve` and `load` (asynchronous, as in Rollup), and `emitFile`, `getFileName`, `setAssetSource`, `parse`, `getModuleInfo`,
  `getModuleIds`, `getWatchFiles`, `warn`, `info`, `debug`, `error` and `addWatchFile`. The ones that answer at once make the hook wait
  for the build (the objects are held by a worker thread of your process, and the hook blocks on a shared flag while it asks).
  `configureServer(server)` gets `middlewares.use` (the whole request, body buffered, goes to your process and the whole response comes
  back), `ws.send`, `config` and `root`. Other methods and the rest of Vite's server (`watcher`, `httpServer`, `moduleGraph`) are not there
  and say so: such a plugin goes in a config file, where it runs in the host itself. `{ module, export, options }` loads a plugin from a module where the build runs, with
  everything a config file's plugin has. These come after the plugins of the config file, which still applies.
- `rkp serve ... --https` (`https: true` in the JS API) serves over HTTPS: a Node.js process (so Node is needed, as for
  plugins) speaks TLS on the public port and passes requests, event streams included, to the dev server on the loopback.
  By default a self-signed certificate for `localhost`, `127.0.0.1`, `::1` (and `--host`, when it is a name or address) is
  made and kept in the cache folder (`$XDG_CACHE_HOME/rathkit` or `~/.cache/rathkit`; `RATHKIT_CERT_DIR` changes it),
  valid for a year and made again when it is about to end; the browser asks you to trust it once. `--https-cert FILE
  --https-key FILE` (PEM; `https: { cert, key }`) use your own, such as one from mkcert. Browsers get HTTP/2 (the many module requests share one connection, and the event stream is a
  stream of it); other clients stay on HTTP/1.1.
- Module federation: an application offers modules to others (`exposes`), takes modules from other applications
  (`remotes`), and agrees with them on one copy of each package they have in common (`shared`). The options are
  those of `ModuleFederationPlugin`; give them as `--federation FILE.json`, as `federation` in `rathkit.config.*`, or
  by putting the plugin from `@module-federation/enhanced/rspack` / `webpack` (or `federation()` from
  `@module-federation/vite`) in the config's `plugins`: the plugin is only read for its options, the bundler does the
  work. It needs the esm format and `--outdir`; the output is split. The runtime that does this is built in
  (`@module-federation/runtime` and `@module-federation/enhanced/runtime` import it, so there is one instance), and
  it speaks the container protocol of webpack, rspack and the Vite plugin. Checked in a browser: a host built here loads a
  remote built by rspack, and a host built by rspack loads a remote built here; a host built here also loads a remote
  built by Vite with `@module-federation/vite`. (`federation()` from that package is not run: its options are read and
  the bundler does the work, so options it has beyond those of `ModuleFederationPlugin` are ignored.)
  - `exposes` become chunks that the container (`remoteEntry.js`, or an ES module with `library: { type: "module" }`;
    `var`, `window`, `self`, `global`, `this`, `assign`, `commonjs`, `umd`) loads on request, and `mf-manifest.json`
    lists them with the shared packages and their files (`manifest: false` writes none, `manifest.fileName` renames it).
  - `remotes` take `name@url`, `script url`, `{ external, shareScope, type }`, manifest URLs (`.json`), and code:
    `promise <expression>` / `var <expression>` give the container (or a promise of it) and `internal <module>` is a module of
    your application. `remoteType` (or `type` on one remote) says how an entry is loaded: a script that defines a global (the
    default), an ES `module`, `commonjs`, a SystemJS module (`system`), or a script that calls a function named like the
    remote (`jsonp`). `import x from "remote/Button"`, `require` and
    `import()` all work, and named imports need no list of exports. A static import of a remote waits for it to load before the
    importing code runs, and so does any chunk with application code in it.
  - `shared` takes `singleton`, `requiredVersion` (from your `package.json` when not given), `strictVersion`, `eager`,
    `version`, `shareScope`, `import: false` (only consume), and a key ending in `/` for everything below a package
    (`lodash-es/`). `include` / `exclude` (`{ version, request, fallbackVersion }`) leave a copy out of sharing, so this
    application uses its own. `treeShaking` also writes the package with only the exports the application reads, in a file of
    its own (an ES module package; other shared packages it reads are shared as usual), which is used when it has all that is
    read and the full copy is not fetched. While watching, the copy is built again only when a file it is made of changes. Every copy on
    offer is put in the share scope and one is chosen by `shareStrategy` (`version-first`, or `loaded-first`), with npm's
    version range rules (checked against `semver`). A shared package that reads another (React DOM reads React) is loaded
    after it.
  - `runtimePlugins` (`"./plugin.js"` or `["./plugin.js", params]`) run in the runtime with the hooks of the other
    runtimes: `beforeRequest`, `afterResolve`, `onLoad`, `errorLoadRemote`, `beforeLoadShare`, `resolveShare`,
    `loadEntry`, `createScript`, `fetch` and more.
  - `dts`: a remote with TypeScript exposes publishes `@mf-types.zip` (and `@mf-types.d.ts`, the unpacked folder) beside
    its manifest, written with the project's `typescript`; a host with a `tsconfig.json` reads the zip of each remote
    that is up into `@mf-types/` and makes `remote/expose` found by the editor through a folder of the remote's name in
    `node_modules` (not where an installed package has that name). Nothing in the project's settings changes;
    `dts: { consumeTypes: { updateTsConfig: true } }` also adds a `paths` entry per module to `tsconfig.json`. `dts: false`,
    or `generateTypes` / `consumeTypes` set to `false`, turn either half off. While watching, types are written and read
    beside the build.
  - `getPublicPath` (the body of a function that returns the address the container's files are served from) is used for the
    container. `dts.generateTypes` honors `outputDir`, `generateAPITypes`, `additionalFilesToCompile` and `abortOnError` (a type
    error stops the build; by default it is a warning), and `dts.consumeTypes` honors `timeout`, `maxRetries`, `remoteTypeUrls`
    (an object of `{ zip, api }` by remote name) and `abortOnError` (types that cannot be read stop the build). `dev.disableLiveReload`
    stops pages from reloading when a served remote builds again, and `dev.disableHotTypesReload` stops types being read again while
    watching. Settings of how the real plugin is arranged inside webpack or rspack (`runtime`, `implementation`, `async`,
    `virtualRuntimeEntry`, the `treeShaking*` directories, `deleteTypesFolder`) change nothing a program can see and are accepted.
    `generateTypes` also honors `compilerInstance` (`tsc`, or the command of another compiler such as `vue-tsc`, run on a project file
    written for it), `extractThirdParty` (the declarations of the packages the types import travel in the zip, minus `exclude`),
    `extractRemoteTypes` (so do the types this application has read of its own remotes) and `afterGenerate` (a function in
    `rathkit.config.*`, called with the files written; it is run from its source text, so it can use only what it defines). `consumeTypes` also honors
    `family` (4 or 6), `remoteTypesFolder` (the archive's name when a manifest does not say), and `consumeAPITypes` with `runtimePkgs`
    (an `index.d.ts` in the types folder that types `loadRemote`). Unlike the real plugin, where `abortOnError` is on by default, a type
    error or an unreadable remote is a warning unless `abortOnError` is set. `bridge` works as in the real plugin: with
    `@module-federation/bridge-react` installed and not turned off, `react-router-dom` is read through the bridge's router for the
    version of React Router the project has (and cannot be shared). The one setting that is named in a warning is `dts.implementation`,
    since another implementation of the types plugin is not run.
  - `serve` does the same in development: the build is in memory, `remoteEntry.js` and the manifest are served to pages
    of other local dev servers (`--cors-origin ORIGIN` adds other origins), and a change rebuilds the remote and
    reloads the pages of hosts that use it.
  - `@module-federation/vite`'s `federation()` is read for its options by default. With `--real-federation-plugin`
    the plugin itself is run by the plugin layer, and its remote loads in a host built here.
  - A package with `treeShaking` is shared whole (with a warning) when it is CommonJS. `promise`, `var` and `internal`
    addresses are code from the config, used as written. A regular expression in `include` / `exclude` `request` keeps its
    flags. Types read by a host are kept up to date while watching: remotes are asked again (at most every ten seconds), a zip
    that has not changed leaves the files alone, and the folders in `node_modules` are put back after packages are installed.
    Under `nodenext` the types are found from ES and CommonJS projects alike.
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
rkp bundle ENTRY... [--outfile FILE | --outdir DIR --splitting] [--no-treeshake]
    [--format esm|cjs|iife] [--global-name NAME] [--global SPEC=NAME]...
    [--minify | --minify-whitespace --minify-identifiers --minify-syntax] [--keep-names | --no-keep-names]
    [--sourcemap[=inline|external]] [--no-sources-content]
    [--config FILE | --no-config] [--mode NAME] [--watch]
rkp serve ENTRY... [--port N] [--host H] [the options above] [--define KEY=VALUE]... [--loader .ext=NAME]... [--asset-names T] [--public-path P] [--external NAME]... [--condition NAME]... [--main-field NAME]...
    [--alias NAME=PATH]... [--tsconfig FILE | --no-tsconfig]
    [--jsx automatic|classic] [--jsx-import-source S] [--jsx-factory F] [--jsx-fragment F]
    [--experimental-decorators] [--decorator-metadata] [--strict-null-checks] [--original-paths] [--platform-browser] [--mode M] [--env-dir D] [--env-prefix P]... [--no-env-files]
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
| Plugins | the same program with the plugins' effects, run by Node: virtual modules, aliases, transforms, hooks in order, emitted files and chunks, module info, `this.parse`, esbuild namespaces, webpack loaders with pitch and inline requests, webpack compiler hooks (lifecycle order, assets, stats, child compiler, built-in plugins), Parcel plugins of every kind, source maps through a transform (Node's own stack traces) | 10 programs x 3-4 output modes (`tools/run-plugins.mjs`); `--watch` (`tools/run-watch.mjs`); the ESTree of every `.js` file in a project's `node_modules` against acorn's (`tools/check-estree.mjs`; 223,000 files on the development machine); real packages as listed under "What is covered by tests" |
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
node tools/run-federation.mjs                 # module federation in Chromium: our host and remote, rspack's, dev servers, types
node tools/test-federation-runtime.mjs        # the federation runtime without a browser
node tools/diff-semver.mjs                    # the runtime's version ranges vs npm's semver
node tools/check-estree.mjs DIR...            # the ESTree of every .js file against acorn's (needs acorn; ACORN_DIR)
node tools/run-loaders.mjs                    # loaders and style sheets (UPDATE_SNAPSHOTS=1 rewrites the CSS snapshots)
node tools/run-defines.mjs                    # --define vs the values set in Node
node tools/test-npm-wrapper.mjs                 # the npm launcher and platform packages
node tools/scan-secrets.mjs [--history]         # secrets in the files, or in every commit ever (the values are not printed)
node tools/check-denylist.mjs [--history]     # the private word list (RATHKIT_DENYLIST) against the files, or every commit
node tools/run-assets.mjs                     # image, font and SVG plugins (url, svgr, file-loader, copy) run for real
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

## Community

Questions and discussion happen on [Discord](https://discord.gg/A5VBxgPaex). It is best-effort and
not a support commitment.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Everyone works from a fork, every commit is signed and carries a DCO sign-off, and the [Code of Conduct](CODE_OF_CONDUCT.md) and [AI policy](AI_POLICY.md) apply. Roles are in [GOVERNANCE.md](GOVERNANCE.md).

## License

Apache License 2.0, see `LICENSE`.
