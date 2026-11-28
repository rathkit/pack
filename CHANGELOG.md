# Changelog

All notable changes to Rathkit Pack are listed here, newest first. The notes of each GitHub
Release are the section of its version.

## Versions

Rathkit Pack follows [Semantic Versioning](https://semver.org) with the usual 0.x reading:
**while the major version is 0, any release may change options, output and plugin support**.
A change that breaks a documented option or flag is listed under "Changed" or "Removed" with the
version in which it happened, and the minor number is raised for it (0.1 to 0.2); patch releases
fix bugs only. Versions with a suffix (`-alpha.N`, `-beta.N`) are published as pre-releases, never as the
latest release, and promise nothing but what the README says is tested.

What an alpha or beta does promise, and checks on every change (`tools/check-promises.mjs`): no dependencies and no install
scripts in any package, a build that needs no network, provenance and checksums on every release. It promises nothing
about stability.

## Unreleased

### Changed

- The minifier cache is on by default (`RATHKIT_CACHE=off`, `0`, `false` or `no` turns it off). A warm minified build writes the same bytes as a cold one and is 9% to 32% faster on the eight workloads measured; the first build, which fills the cache, takes 0% to 13% longer. It writes under `$XDG_CACHE_HOME/rathkit/minify` (or `RATHKIT_CACHE_DIR`) and keeps the folder under 512 MB.
- The minifier leaves out names declared inside a function that nothing reads (a `const`, `let` or `var` with an initializer that has no effect, a function, a class), and then the names only those read. Vue 94,887 to 94,568 bytes and TypeScript 3,545,980 to 3,545,815 on the benchmark workloads; the other thirteen are unchanged.

## 0.5.0-beta.0 - 2026-11-24

### Changed

- The metafile names files outside the working folder with `..` (`../shared/lib.js`) instead of absolute paths, so the same build in two checkouts of one layout writes the same metafile. Its `outputs` are named from the working folder (`dist/main.js`, not `main.js`), list every file the build wrote, and give each a `kind`.
- `build().files` lists every file the build wrote (source maps, declarations, license files, copied files, pages, the metafile) as absolute paths.
- Minified CSS is smaller: longhands are written as their shorthand, hex alpha colors are used, `transparent` is `#0000`, equal values are shorter (`flex`, `transition`, `border`, times, `translate`, `background`), and a rule repeated later in the same list is dropped.
- Faster builds of big graphs: tree shaking finds enums and builds its symbol maps on all cores (the shake phase of the 1,500-module benchmark app is about 40% faster), and dependency discovery wakes a helper thread only for files the busy workers will not take (the graph phase of that app is about 14% faster on 16 cores).
- `--write-exports=FILE` is documented with its copy workflow, and its errors say what to do.

### Added

- `rkp migrate` finds a config file in a subfolder (one a package.json script names, or one found by searching two levels down), not only in the project root.
- `rkp migrate` runs a Rollup config whose input or output is built in code (in Node's permission model, reads only) to read its exact values, and prints a command for each output. `--no-run` turns it off.
- `rkp migrate` names the lines of an esbuild API script that change (the import, the `build()` calls that stay the same, the calls and options `@rathkit/pack` does not have).
- `rkp migrate` follows a Vite `base` to a constant or a `.env` variable and writes `--public-path`, maps `build.lib` to a command for each format, and maps a multi-page `rollupOptions.input` to several entries, pages that share a file name included.
- `--list-files=FILE` writes the files a build wrote, one per line.
- `--ready-file=FILE` for `rkp serve` and `--watch` says when the server or the watcher is up, and `--port=N` sets the port of the server; the ready lines are documented.

### Fixed

- A page that links a style sheet which its script also imports stopped with `no built style sheet`; it builds now, with the sheet written once. A page build no longer prints (or lists in the Node API) the empty script of a style sheet it deleted.
- A page build could leave a script's style sheet unlinked when its name ended another sheet's name.
- Pages with the same file name in different folders (`index.html` and `nested/index.html`) are written at their own places in the output folder instead of one overwriting the other.
- `rkp migrate` no longer prints a pattern of `rollupOptions.input` (`pages/**/*.html`) as a page, and finds the page beside a Vite config that is in a folder (`demo/vite.config.js` and `demo/index.html`).
- `--entry-names` was ignored for the one file of a build with a single entry and no `--splitting`; the names are now the same with and without `--splitting`.

### Known gaps

- CSS minification does not merge rules that are not next to each other or reorder declarations (`#23`): on 9 of the 15 corpus style sheets Pack is 2 to 25 gzip bytes above the smallest other tool.
- Function inlining across modules (`#126`) and dead-parameter removal (`#127`) are not done: measured on the benchmark workloads they gain nothing in gzip size, and a constant table across the pieces of a big minified bundle (about 2% raw and 4% gzip on lodash-es) is not built yet.
- `--write-exports` still needs a file that exists, and `rkp serve` and `--watch` have no `--json` stream of events (`#134`, `#136`).
- A depfile (every file a build read, including files probed and not found; `#131`) is not in this release.
- The corpus run of this release (run 45353352139) builds and checks 59 of the 63 projects. `tanstack-query-basic` and `tanstack-query-simple` do not install, because pnpm 11's supply-chain policy stops on `solid-js@1.9.20` ("high-risk trust downgrade"): that is the package's publication, not a Pack failure, and the two projects built in the corpus of 0.4.0-beta.0. For `preact` and `signals` the command that `rkp migrate` prints does not build: their Vite configs are in a folder (`demo/`, `extension/`) and set a root with `__dirname`, or name their entries with `resolve(__dirname, ...)`, which migrate does not follow (it says so in a note). The commands it prints for the other 28 projects that it has a command for build, and it finds nothing to say about the remaining 31.
- On the container benchmark of this release (docs/benchmarks.md), Pack is ahead of esbuild on time and memory in all 45 build cases, as in 0.4.0-beta.0, and no case that Pack won on time became a loss or a tie. The machine measured every tool 3 to 5% slower on that day than on the day of the 0.4.0-beta.0 run (esbuild 5%, Pack 5%); run back to back in one session, the 0.4.0-beta.0 build and this one differ by -1.6% (geometric mean over 30 cases of plain and minified builds), and the 1,500-module app builds about 17% faster.

## 0.4.0-beta.0 - 2026-11-19

### Changed

- Smaller minified output. A namespace path such as `ns.a.b`, where `a` is a namespace a module re-exports (`export * as a`), is folded to the
  binding it names across modules, and a call of a function that only returns a value is dropped when its result is unused. The minifier writes a
  `const` that is read once where it is read, drops an `else` after a branch that always leaves, and lets `export default x` alias a variable that is
  initialized once. In the pack-bench container, minified Zod is about 6.4 KB smaller than with 0.3.0-beta.0 (gzip about 1.9 KB), D3 3.9 KB, the TypeScript compiler
  2.0 KB, Solid 0.6 KB and lodash-es 0.8 KB, with the same behavior on all 15 workloads.
- Smaller minified style sheets (without a source map): `#ffffff` is `#fff`, `0.5` is `.5`, trailing zeros and `0px` go where a length is certain, the `bold`
  and `normal` weights are 700 and 400, `rgb(r,g,b)` is written in hex, `rotateZ`, `skewX` and a mirroring `matrix` are shorter, quotes come off font names
  that are plain words, duplicate parts of `margin`, `padding` and similar shorthands are dropped, the space before `!important`, after `@media` and around
  `~` and `+` goes, `::before` is `:before` (also `after`, `first-line` and `first-letter`) and `[a="b"]` is `[a=b]`. Custom properties, strings and `url()`
  are left as written. A rule that follows one with the same selector is one rule, and one with the same body joins its selector list when both selectors are
  of the kind every browser reads. A quoted `url()` that holds a `)` no longer ends the scan early. The minified text of style sheets is different from before.
- Faster builds of big bundles: the modules of a bundle of 256 KB of source or more are scanned on all cores (the output is the same for every number of
  cores), and a source map's line tables and inline-map lookups are made on all cores, so a map adds 15% to a minified build of lodash-es instead of 39%. The scan keeps the records of the modules it is working on, which costs a
  little memory on mid-size bundles: the peak of React server rendering with a source map goes from 19.3 to 19.9 MB on four cores (3%; 10% on a hosted CI runner).
- Less memory: a one-shot build drops the per-module scans and used-name sets before the minifier starts, finds repeated requests through an index
  instead of a quadratic scan, and runs on a 1 GB stack so that deeply nested generated files do not overflow it. `docs/memory.md` states the bound and
  `tools/run-worstcase.mjs` (50,000-module graphs in four shapes, a 16 MB module and a barrel of 5,000 re-exports) checks it on Linux. `RATHKIT_TIMING`
  prints the resident and peak size of each phase.

- An unknown `--format` (`umd`, `amd`, `system` or a misspelling) is an error that lists the formats that exist; it was read as `esm` and built without a word.
- `--external` takes one `*` anywhere in the name (`@scope/*`, `*.png`), and a package name also covers its subpaths (`react/jsx-runtime`). A build that would
  write over one of its own inputs stops, as esbuild's does; `--allow-overwrite` lets it.

- webpack configs: `resolve.extensions`, `devtool`, `output.publicPath` and `optimization.minimizer` are read and honored, the hooks of `RealContentHashPlugin` and
  chunk group iterables exist for subresource-integrity style plugins, and more `output.*` defaults are present for plugins that read them.
- webpack `optimization.splitChunks.cacheGroups` now decide the chunk layout (with `--splitting`). `test` (a RegExp, a string or a function, all run in the plugin host
  in one call), `name`, `chunks` (`initial`, `async`, `all`), `priority`, `minChunks`, `minSize` and `enforce` are followed: a group with a `name` makes one file with that
  name, one without makes a file for each set of entries and lazy chunks that need its modules. An entry, or a module `import()` loads, never leaves its own file. A group
  that would make chunks that import each other is given up with a warning that names it. Only the groups a config lists apply (webpack's implicit `defaultVendors` and
  `default` do not), and a build without cache groups writes the same bytes as before. `maxSize` and the other size-splitting settings, `runtimeChunk`, `moduleIds` and
  `chunkIds` do not apply to Pack's output (`docs/webpack-conformance.md` says why).
- Plugin host, Rollup and Vite plugins: `getModuleInfo(id)` is known while transform hooks run, so `@rollup/plugin-commonjs` runs; an `options` hook that replaces
  `input` is honored for the entries and for `buildStart` (`@rollup/plugin-multi-entry`); a chunk's `modules` leaves out modules that were shaken out entirely, as Rollup's
  does (`rollup-plugin-import-css` and `rollup-plugin-styles` decide what to emit from it); config hooks get a config with `build`, `server`, `preview` and
  `optimizeDeps`, and `build.rollupOptions.external` from a config hook is honored; the resolved config has `preview`, `createResolver` and the plugin list; a hook filter
  given as a glob matches as a glob; and a loaded module whose code opens with an assignment is not read as the file's own type (`vite-plugin-wasm`).

- The minified text of a bundle no longer depends on the number of cores. A bundle of 96 KB of text or more is cut into 32 pieces on every machine; it was cut by the
  number of threads, and not at all on one core, so the same bundle gave a different text on one, two, four and sixteen cores (lodash-es was 1.6 KB smaller on one core, three.js
  differed at every count). A machine of eight threads writes what it wrote before, so the benchmark figures stand; others may see a few bytes of difference from
  0.3.0-beta.0.

### Added

- An optional cache for the minifier, off unless `RATHKIT_CACHE=on` (or `1`, `true`, `yes`): a one-shot minified build keeps what the minifier made of each chunk of 16 KB
  or more, so a warm build skips that step (TypeScript's 9 MB bundle builds about a third faster) and writes byte for byte what a cold build writes. An
  entry holds the chunk's text, the settings, the layout and the program that made it next to the answer, and a hit needs every one to match; a missing,
  cut short, damaged or stale entry is ignored and written again, and a folder that cannot be written is not an error. The folder is `RATHKIT_CACHE_DIR`, else
  `$XDG_CACHE_HOME/rathkit/minify` or `~/.cache/rathkit/minify`, kept under 512 MB. Watch mode and the dev server do not use it.

- `rkp analyze meta.json [--outfile report.html]` reads a metafile (`--metafile=FILE`) and writes one self-contained HTML page: a treemap by package, the packages
  and modules of each output with their sizes, what imports a module, and a filter box. It fetches nothing and needs no server.
- `rkp migrate [DIR] [--write]` finds the build tool of a project (esbuild, Vite, webpack, rspack or Rollup) and prints the equivalent rkp commands, with notes on
  what does not carry over. `--write` adds `build:rkp` and `dev:rkp` to the scripts of `package.json` and never changes a script that is there. Run on the 63 corpus
  projects, it found the tool of all 34 that have one, and every command it printed that can run builds the project (`docs/migrate/corpus-check.md`).
- Migration guides from esbuild, Vite, webpack and Rollup (`docs/migrate/`), each starting from a small project of the tool and checked in CI (`tools/run-migrate.mjs`).
- esbuild's spellings of options are read (`--outfile=FILE`, `--external:NAME`, `--alias:A=B`, `--conditions=a,b`, `--main-fields=a,b`, `--platform=`,
  `--jsx=transform|automatic`, `--tree-shaking=`, `--sources-content=`, `--bundle`, `--charset=utf8`, `--log-level`, `--color`, `--log-limit`), and the npm API accepts
  `entryPoints`, `absWorkingDir`, `bundle: true`, `charset: 'utf8'`, `logLevel`, `color`, `logLimit` and `allowOverwrite`. `docs/esbuild-conformance.md` lists every esbuild
  option with its status and a probe that CI runs (`tools/run-esbuild-conformance.mjs`, 114 probes).

- Conformance tables with a fixture for each supported row, built with the real packages: `docs/webpack-conformance.md` (configuration options, the plugins that ship with
  webpack, the 30 most used loaders and plugins, the `SplitChunksPlugin` settings and the plugins that read the compilation; 75 fixtures) and
  `docs/rollup-vite-conformance.md` (53 Rollup and 50 Vite plugins; 103 fixtures). They run in CI (`tools/run-webpack-conformance.mjs`,
  `tools/run-rollup-vite-conformance.mjs`).

- The README states what Pack writes, changes and deletes ("What Pack writes and deletes"), and `tools/run-writes.mjs` checks it in CI on Linux: it runs each command in a sandbox
  with its own home, cache and temporary folders, compares the whole folder before and after, and fails on anything unstated. One thing is written outside the project
  without a name: `rkp serve --https` keeps a certificate and key for `localhost` in the user cache folder.

### Known gaps

- Source maps still add 20 to 25% to a minified build on the TypeScript compiler, three.js, React, rxjs, Zod and the 500-module TypeScript app (#25).
- CSS minification does not build shorthands from longhands, merge rules that are not next to each other, or change `rgba()` to hex (#23).
- The cache does not make the development server or watch mode start faster; their time is in parsing and scanning (#26).
- A bundle of 96 KB of text or more is minified in pieces, and a constant declared at the top level of a piece is not written at its one read, because
  another piece may read it. A whole-bundle count of reads would allow it; lodash-es is about 1.6 KB larger without it.
- Of esbuild's options, the ones Pack does not support are listed in `docs/esbuild-conformance.md`; among them are the formats `umd`, `amd` and `system`.
- `SplitChunksPlugin`: `chunks` and `name` as functions, `filename`, `reuseExistingChunk`, the request limits (`maxAsyncRequests`, `maxInitialRequests`) and webpack's implicit
  default groups are not followed; the modules a group takes run before the rest of the chunk they came from (webpack keeps the original order); the Windows path form of a group's `test` is written but only Linux ran it. The other gaps of the webpack, Rollup and
  Vite plugin tables are listed in `docs/webpack-conformance.md` and `docs/rollup-vite-conformance.md`.
- On the container benchmark of this release (docs/benchmarks.md), Pack is ahead of esbuild on time and memory in all 45 build cases, on minified size and gzip in all 30 minified
  cases, on warm rebuild in all 30 cases and on HMR latency in all 14. Against the other tools, one time win of 0.3.0-beta.0 is gone: the warm minified rebuild of D3 is 9.7 ms for Pack,
  as before, and 7.7 ms for Parcel 3, which was 9.9 ms in that run (Parcel 3's own runs vary by that much). Builds of very small projects take about 1 ms longer than with 0.3.0-beta.0 (date-fns
  minified 14 to 15 ms, the monorepo plain 11 to 12 ms; esbuild takes 27 and 21 ms): the output-size work above costs 0.3 to 0.4 ms of it, and a check that the build does not overwrite
  its own input cost 1.2 ms more until it was narrowed before this release.

## 0.3.0-beta.0 - 2026-11-16

### Changed

- Faster builds of mid-size programs, most of all with `--minify --sourcemap`: the tree shaker reads the modules side by side, a bundle
  from 96 KB of text is minified in pieces (it was 192 KB; the names inside a piece are chosen apart, so a bundle of that size can come out
  a few bytes different, with the same behavior), and on Linux with glibc the allocator takes the heap in steps of 2 MB and keeps what it took
  instead of asking the system for 128 KB at a time. Peak memory is unchanged.
- Watch mode and the development server hear of a change at once on Linux (inotify, called through the C library, no dependency)
  instead of at the next 80 ms poll and a 40 ms wait; the change is taken after a 4 ms quiet spell. A build keeps, for every module
  whose file did not change, the places its imports were found, so a warm rebuild looks nothing up again for them. Elsewhere
  (macOS, Windows, or when inotify has no watch left) the files are polled as before; FSEvents, kqueue and `ReadDirectoryChangesW`
  need a run loop or overlapped I/O that cannot be had without a dependency.
- `rkp serve --hmr` tells the page of a module update as soon as the modules are written, before the source map and the other files of the
  build are made (the files wait for the build, so a page that loads then does not get the code from before), and a watcher starts a
  build at once when the writer of a changed file has closed it instead of waiting 4 ms for quiet. The wait for file events lasts until
  the next full look, and a signal ends it through a pipe. When a build is run again (watch, serve), the scan of a module that imports
  nothing from the bundle (a CommonJS library, a big vendored file) and the pieces a big bundle is minified in are kept for the next
  build and used when nothing they depend on changed, which takes the warm minified rebuild of the TypeScript compiler from over a hundred
  milliseconds to a few tens. The output is the same as that of a build from nothing (`tests/rebuild.rs`, `tools/run-rebuild.mjs`).

### Known gaps

- Pack does not read `base` from a Vite configuration; its default public path is `./`.
- On the container benchmark of this release (docs/benchmarks.md), Pack is ahead of esbuild on time and memory in all 45 build cases, on minified size in every
  minified case, on warm rebuild in all 30 cases and on HMR latency in all 14. esbuild is still smaller in two plain cases (React by gzip, Zod by size). Vite's
  HMR is faster everywhere and Parcel's on most workloads; Parcel 3 has the lower warm rebuild on half the cases. macOS and Windows still poll for file events.

### Added

- Style sheets without a configuration file: when a project has a `postcss.config.*`, a `postcss` field in `package.json`, or a Sass, Less or Stylus
  package installed, those are run for style sheets only (JavaScript modules are not sent through the plugin host); `--no-config` turns it off.
  `@import` conditions, `url()` with a `#fragment` or `?query`, `@charset`, and `@value` and `:export` in CSS modules are handled. The corpus has
  apps for Sass, Less, PostCSS and CSS modules, and one that builds with no configuration at all.
- An optional type checker, `rkp-check`, installed apart from `rkp` (which stays byte for byte the same with or without it), and `rkp check` to run it.
  This is stage 1 only: it reports TypeScript 5.9.3's scanner errors and nothing else, and a run with no diagnostics says so. See `docs/typechecker.md`.
- `--target=es5`, and engines without es2015 (`ie11`, `chrome44`): arrow functions, classes, `let` and `const`, templates, destructuring, default and
  rest parameters, spread, `for of` (closing its iterator), new object literal forms, private members, `new.target`, generators, async functions,
  async generators and `for await` (a state machine) and the output's own helpers are written as es5. A browserslist input: `--target` defaults to the queries of the project (`.browserslistrc`, `package.json`, `BROWSERSLIST`) or of
  `--browserslist`, for the queries that name engines and versions.
- Library mode: `--format esm,cjs` writes both formats in one build, `--exports auto|named` chooses how a CommonJS entry hands out its exports, `--preserve-modules`
  (and `--preserve-modules-root`) writes one file for each module, and `--write-exports` sets `main`, `module`, `types` and `exports` in the `package.json`.
  The corpus libraries are built in both ways and compared with the releases their authors publish.
- `--dts-bundle` and the types of module federation work with TypeScript 7: its command line writes the declarations and a reader of our own merges them.

- Smaller minified output: unused declarators of a shared declaration are dropped, more built-in reads count as pure, private names get short names,
  and `else if` chains, trailing guards, `typeof` comparisons and nullish checks are written shorter. On the 15-workload benchmark Pack's minified
  output is smaller than esbuild's on Preact, Svelte and date-fns; it is still larger on Solid (+1.7%) and Zod (+0.4%), though smaller when gzipped.

### Fixed

- `@vitejs/plugin-legacy` no longer fails the page with "No corresponding legacy polyfill chunk": it builds a second SystemJS bundle in a second Rollup pass, which Pack does not run, so its plugins are skipped with a warning, `import.meta.env.LEGACY` is `false`, and `--target es5` (or a browserslist query) lowers the syntax of the one bundle.
- `--dts-bundle` follows a re-export written with a `.ts` extension (`export { A } from './a.ts'`), which it left out.

## 0.2.0-beta.0 - 2026-11-11

The first beta: measured and gated. The benchmark suite has 15 workloads with warm-rebuild, development server and type-check
measurements, CI fails on a regression in output size or peak memory, and the corpus has 60 projects.

### Added

- The benchmark ([rathkit/pack-bench](https://github.com/rathkit/pack-bench), figures in [docs/benchmarks.md](docs/benchmarks.md)) has
  15 workloads (Preact, Vue, Solid and Svelte apps, a monorepo-style TypeScript app, date-fns, Zod and D3 beside the original seven)
  and times warm rebuilds in each tool's watch mode, development server start and HMR, and build plus type check for the
  TypeScript workloads. Pack took the least memory in all 45 build cases and less time than every tool but esbuild and Parcel 3, which
  were faster in 3 cases each. In watch mode a Pack rebuild is not the fastest, and HMR notices a change in 85 to 318 ms because it
  polls; both are listed as gaps. There is no type-check figure for Pack, which has no checker yet. Against the previous published run, Pack's medians on the seven original workloads moved by -3% to +12% with
  identical output and memory, and Pack is still ahead of esbuild in each of them.
- The corpus grew from 24 to 60 pinned projects (`corpus/projects.json`): more libraries (unjs, pmndrs, nanostores, chalk, d3,
  preact and others), and Vue, Solid, Preact, React and Svelte apps, among them Vue apps with UnoCSS and Sass, Redux and
  Material UI templates, and folders of larger repositories. All 60 build, and the apps load in a browser without errors.
- Sass finds a package by its name, as Vite does: `@use 'element-plus/theme-chalk/src/index.scss'` is looked for in the
  `node_modules` folders above the file. A leading `~` works too.
- CI checks the output size and the peak memory of the bundler against a stored baseline (`tests/baseline.json`, `tools/check-baseline.mjs`):
  five workloads built plain, minified and minified with a source map; a figure that grows past its tolerance fails the build. Time is
  checked by the maintainer before a release ([docs/RELEASING.md](docs/RELEASING.md)).

### Fixed

- `export default function() {}` (no space after `function`) in a module that another module imports by name was written as
  `functioncluster_default () {}`, which does not parse. Found building d3-hierarchy.
- A Vite configuration that reads `__dirname`, `__filename` or `require` at load time (as most written for CommonJS do) is
  bundled with their values instead of failing with "is not defined in ES module scope". Found building vitesse-lite.
- The style block of a single-file component (`App.vue?vue&type=style&index=0&lang.scss`) is compiled as Sass, Less or
  Stylus by the language its query names, not left as source for the style sheet reader.

## 0.1.0-alpha.2 - 2026-11-09

Every npm package and the crate are now listed, with the binaries, in the signed `SHA256SUMS` of the release, and the `npm` and
`crates` jobs of the workflow stop unless what they publish matches it; `tools/approve-release.mjs` makes the same check before the
owner approves ([docs/RELEASING.md](docs/RELEASING.md), [docs/verifying.md](docs/verifying.md)).

### Added

- `--dts` and `--dts-dir` work with TypeScript 7, which has no JavaScript API: its command line writes the declarations. `--dts-bundle`
  still needs TypeScript 5 or 6 and says so.
- The browser checks of the repository (the development server and `--hmr` with React, Vue and Svelte, and module federation) run in Firefox and
  WebKit as well as Chromium (`BROWSER=firefox` or `webkit`); the three are driven with no dependency of ours and are run in CI on Ubuntu 24.04.
- Less, Sass (`sass-embedded` or `sass`) and Stylus files are compiled when the compiler is installed in the project, as Vite does,
  and read as style sheets (including `.module.scss`). This applies to builds that use a configuration file, since the
  compilers run in the plugin host; a project that lists its own plugin for one of these languages keeps it.

### Fixed

- Watch mode finds a file that changed while the build that first saw it was running.
- The self-signed certificate of the HTTPS dev server has a serial number that is a valid DER integer.
- On Windows, a path is written as a file URL with `--original-paths`, a name that starts with a slash stays rooted, and the
  in-memory modules of a build, the metafile and module federation compare paths without the platform's separators.
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
