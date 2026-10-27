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

## 0.1.0-alpha.0

First release.

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
- JS API: `plugins` takes plugin objects (their hooks run in the calling process, across a local socket) and `{ module }` descriptors.
- `rkp serve --https` (with `--https-cert` and `--https-key`, or a self-signed certificate it makes and keeps): TLS through a Node.js proxy in front of the dev server.
- `rkp serve --hmr`: changed modules are replaced in the open page (`import.meta.hot.accept`, `dispose`, `data`, `decline`, `invalidate`), with the page reloading only when a change reaches an entry nothing accepts.
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
