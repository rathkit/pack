# Changelog

All notable changes to Rathkit Pack are listed here, newest first. The notes of each GitHub
Release are the section of its version.

## Versions

Rathkit Pack follows [Semantic Versioning](https://semver.org) with the usual 0.x reading:
**while the major version is 0, any release may change options, output and plugin support**.
A change that breaks a documented option or flag is listed under "Changed" or "Removed" with the
version in which it happened, and the minor number is raised for it (0.1 to 0.2); patch releases
fix bugs only. Versions with a suffix (`-alpha.N`) are published as pre-releases, never as the
latest release, and promise nothing but what the README says is tested.

What an alpha does promise, and checks on every change (`tools/check-promises.mjs`): no dependencies and no install
scripts in any package, a build that needs no network, provenance and checksums on every release. It promises nothing
about stability.

## Unreleased

### Added

- Less, Sass (`sass-embedded` or `sass`) and Stylus files are compiled when the compiler is installed in the project, as Vite does,
  and read as style sheets (including `.module.scss`). This applies to builds that use a configuration file, since the
  compilers run in the plugin host; a project that lists its own plugin for one of these languages keeps it.

### Fixed

- In a page build, the dev server and `--platform-browser`, `process.env.NODE_ENV` is a constant (`production` for a build, `development`
  for the server, or the environment's `NODE_ENV`; `--define` wins), as in Vite. Those outputs also no longer start with
  `import ... from "node:module"` or `"node:url"` (the `require` and `__dirname` shims), which a browser cannot load and which
  stopped every chunk of a page that had any CommonJS code in it.
- In a page build, a Node built-in (`crypto`, `fs`, `node:path`, ...) that no plugin supplies is an empty module, as in Vite, so a library that
  reaches for `crypto` when it is there no longer leaves `import "crypto"` in a script a browser cannot load. A program for Node keeps the import.
- In a page build (and `--platform-browser`), the `browser` condition of `exports`, the `browser` field as an entry, and the object form of
  a package's `browser` field (`"./lib/node.js": "./lib/web.js"`, `"fs": false`) are followed, as in Vite; axios and many other packages need it
  to keep their Node code out of a page.
- When the build is split into chunks, a member read through a namespace import (`import * as N from "pkg"; N.make()`) whose declaration is
  behind files of re-exports (`export * from`) is kept; it was left out of every chunk (naive-ui's `createDiscreteApi`).
- Code written without semicolons: a name that the bundle writes as `(0, x.y)` or `(x)` (a CommonJS import) no longer joins the line before it
  as a call (`const a = f()` followed by a line `useEffect(...)` became `f()(0, ...)`); the `;` the author left out is written.
- A dynamic import with a variable may start with an alias (`import(\`@/layouts/${name}/index.vue\`)`, as Vue's `defineAsyncComponent` is
  usually written), and the variable may be in the name of a folder (`${name}/index.vue`), not only in the last name; the extension must still be written out.
- A build driven by a `webpack.config.*` or `rspack.config.*` names the chunk of the default entry `main.js`, as webpack and Rspack do, not
  `bundle.js`, so a page template that says `<script src="./main.js">` finds it.
- Rspack's `HtmlRspackPlugin` (from `@rspack/core`) works: the compiler takes Rspack's built-in plugins through `__internal__registerBuiltinPlugin`,
  and writes the page (the template with `<%= htmlWebpackPlugin.options.title %>`, `meta`, `base`, `inject`, `scriptLoading`, `filename`) with a
  `<script>` and `<link>` for each initial chunk. Another built-in plugin is named in a warning.
- A JSX spread child (`<List>{...items}</List>`) builds in the automatic runtime (`children: [...items]`).
- A decorator on a class field uses the output's names for what it refers to (it kept the source name when the bundler had renamed an
  import, as with Lit's `@property`).
- A script may import a file of `public/` by its address (`import icons from '/icons.svg'`, which a Vue template with `<use href="/icons.svg#a">`
  becomes): the project folder is tried, then `public/`, when no such file is on the disk.
- `import.meta.glob` patterns may start with an alias (`@/views/**/*.vue`), from `--alias` or the configuration's `resolve.alias`
  when it maps a name to a path.
- A configuration file that imports another file without its extension (`./config/options`, usual in a TypeScript `vite.config.ts`)
  is bundled first, as Vite does, when Node cannot follow the import.
- A part of a file that a plugin names by language (`Page.vue?vue&type=script&lang.ts`) is read as that language, so the
  TypeScript in a Vue `<script setup lang="ts">` block builds.
- A TypeScript function overload signature inside another function is removed from the output (it was left as a
  bodiless `function f(obj)`, a syntax error).
- In a TypeScript file, `export { Draft } from "./x"` of a name that module does not export is taken to be a type and left out
  of the output, as `tsc` and esbuild do, and no longer an error.

## 0.1.0-alpha.1 - 2026-11-01

The program is the same as in 0.1.0-alpha.0. This is the first release whose npm packages and crate were published by the
release workflow, with provenance: check them as [docs/verifying.md](docs/verifying.md) describes.

## 0.1.0-alpha.0 - 2026-10-31

First release.

The registries only accept trusted publishing from a package that already exists, so the npm packages and the crate of this
first version were published by hand, **without provenance**. Their files are the ones attached to this release: compare them
with `SHA256SUMS` as [docs/verifying.md](docs/verifying.md) describes. Every later release is published by the release workflow, with provenance.

### Added

- A bundler for JavaScript, TypeScript, JSX, JSON and CSS with no dependencies (not even crates):
  module graph, linking, tree shaking, code splitting, minifying, source maps, defines, loaders
  (text, base64, data URL, binary, file, empty, CSS and local CSS) and `?raw`, `?url` and `?inline`
  import suffixes.
- Plugins: Rollup and Vite plugins, esbuild plugins, webpack loaders and compiler plugins, and
  Parcel plugins, run by a Node 24 or later process. What is covered by tests is listed in the README.
- Module federation: containers, remotes, shared packages, the manifest, generated and consumed
  types, runtime plugins and development servers, compatible with the containers rspack and
  Vite build (tested in Chromium).
- `--watch` and `serve` (a development server with live reload). Stopping `--watch` cleanly with Ctrl-C is tested on
  Linux and macOS only.
- Release files: one tarball per platform, `SHA256SUMS`, an SBOM that lists zero components, and
  Sigstore signatures. The binaries are not signed with an operating-system key; see
  [docs/verifying.md](docs/verifying.md).
- The command is `rkp`. `rkp --version`.
- Pages: `rkp bundle index.html --outdir dist` builds the module scripts and linked style sheets a page names, copies the
  images and media it names and `public/`, rewrites the page, and runs plugins' `transformIndexHtml` on it (ES modules, or
  one file with `--format iife`).
- `--banner`, `--footer`, `--drop:console`, `--drop:debugger`, `--packages=external`, `--resolve-extensions`,
  `--preserve-symlinks`, `--pure`, `--metafile`, `--entry-names`, `--chunk-names`, `--out-extension`, `--legal-comments`, `--inject`, `--jsx-dev`, `--tsconfig-raw`; an unknown option is an error.
- Module federation: `getPublicPath`, `dts` `outputDir`, `generateAPITypes`, `additionalFilesToCompile`, `abortOnError`, `timeout`, `maxRetries`, `remoteTypeUrls` and `dev.disableLiveReload` / `disableHotTypesReload` are used; what is not carried out is named in a warning.
- Module federation: `compilerInstance`, `extractThirdParty`, `extractRemoteTypes`, `afterGenerate`, `family`, `remoteTypesFolder`, `consumeAPITypes`, `runtimePkgs` and the router `bridge` are carried out; only `dts.implementation` is named in a warning.
- `--target`: object rest is rewritten wherever it is (nested in other patterns, after computed keys, in assignments and `catch`); a return on an async generator suspended in `yield*` ends it; an async arrow with a rewritten body (`??`, `?.`) no longer gets its wrapper twice.
- `--target` takes engines with versions (`chrome80`, `safari13.1`, `node18`, `firefox78`, `ios14.5`, `edge90`, `opera70`), alone or with years in a comma-separated list; only what one of them lacks is rewritten. Class fields, private members and static blocks are rewritten together when an engine lacks any of them.
- `--target`: top-level await is rewritten for targets that lack it when the program is one file with nothing exported (the whole program becomes an async function, or a generator run by `__async` below es2017).
- JS API: `plugins` takes plugin objects (their hooks run in the calling process, across a local socket; `emitFile`, `parse`, `getModuleInfo` and the other context methods that answer at once work, and `configureServer` gets middlewares and `ws.send`) and `{ module }` descriptors.
- `rkp serve --https` (with `--https-cert` and `--https-key`, or a self-signed certificate it makes and keeps): TLS (HTTP/2, or HTTP/1.1 for clients that ask for it) through a Node.js proxy in front of the dev server.
- `rkp serve --hmr`: changed modules are replaced in the open page (`import.meta.hot.accept`, `dispose`, `data`, `decline`, `invalidate`), with the page reloading only when a change reaches an entry nothing accepts; top-level await and decorators work, and React Fast Refresh keeps the state of components (when `react-refresh` is installed).
- Plugins: Vue (`@vitejs/plugin-vue`) and Svelte 5 (`@sveltejs/vite-plugin-svelte`) work, including under `--hmr`; hooks written as objects with a `handler`, `configEnvironment`, the `serve` command, and `lang.<ext>` queries (`?vue&type=style&lang.css`) are understood.
- A normal bundle no longer breaks when a module declares a top-level name the helpers use (lodash-es declares `Symbol`).
- Tested in headless Chromium only: the development server, `--hmr` and federation have not been tried in Firefox or Safari.
- The development server (and the HTTPS proxy) answers on both loopback addresses when the host is `localhost`; before, it listened on the first address `localhost` resolved to, so `http://127.0.0.1:PORT` was refused on machines where that is `::1`.
- A hook of a plugin object that calls a blocking context method (`emitFile`, `parse`, `getModuleInfo`, ...) could wait forever on a machine with few cores; the answer is now awaited without that race.
- JSX: a space between two elements on one line (`<a/> <b/>`) is kept, as Babel and esbuild keep it.
- `--dts-bundle`: one merged `.d.ts` for each entry instead of one for each source file.
- `--dts` and `--dts-dir`: declaration files for the entries, written with the project's `typescript` (JS API `dts`, `dtsDir`).
- `--target=es2015` to `es2025`: `**`, `?.`, `??`, logical assignment, numeric separators, optional `catch` and trailing commas
  and object spread and rest (in declarations, parameters and `for of`), class fields, private members and static blocks are
  async functions (below es2017), `for await` and async generators (below es2018) are rewritten for targets that lack them; what cannot be rewritten stops the build and is named.
- `import.meta.env` with `MODE`, `DEV`, `PROD`, `SSR`, `BASE_URL` and the `VITE_` variables of `.env` files (`--mode`, `--env-dir`,
  `--env-prefix`, `--no-env-files`).
- Dynamic imports with a variable in the path (`import(`./locales/${name}.js`)`), with the rules of Rollup's plugin for them.
- `./a.wasm?init` instantiates a WebAssembly module (a function, as Vite's).
- `import.meta.glob`, and workers (`new Worker(new URL(...))`, `?worker`, `?sharedworker`, with `&inline` and `&url`).
- Builds for linux (x64 and arm64, glibc and musl), macOS (arm64 and x64) and Windows (x64). The unit tests that use fake
  Unix paths run on Unix only; each release build bundles and runs a program with the binary it made.
- A JavaScript API in `@rathkit/pack`: `build(options)`, `serve(options)`, `run(args)`, with types.
- npm packages: `@rathkit/pack` (a launcher) and one package per platform, published with provenance on the `next` tag.
