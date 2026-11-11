# Roadmap

What is left before Rathkit Pack 1.0, in the order it is meant to be done. Each item has a stable ID and an issue, grouped in
milestones; the issues are also in the [Rathkit Pack roadmap project](https://github.com/orgs/rathkit/projects/1) (board, timeline by milestone,
tracks and priorities). A box is ticked in the commit that finishes the item, and the pull request says `Closes #N`. This is a plan, not a
promise: the order changes when real projects show what breaks.

Items marked *(maintainer)* need an action on GitHub, npm or elsewhere that only the maintainer can take.

## What 1.0 means

1. **Drop-in.** A project that builds with Vite, webpack, Rspack or Rsbuild, Rollup or esbuild (or a wrapper such as tsup) builds with `rkp`
   after changing one command, and the result runs the same. Each tool has a conformance suite (its own example projects and its most used
   plugins); 1.0 means every suite passes and each remaining gap is documented.
2. **Fast, small and lean, measured.** A public benchmark suite of at least 15 workloads, and three bars judged for each workload against
   the other tools it benchmarks: median build time at or below the fastest, minified and gzip size at or below the smallest (a difference
   under 0.5% counts as a tie), and the lowest peak memory. A workload where another tool wins is listed in
   [benchmarks.md](benchmarks.md) with the reason and an issue. Public statements follow the suite and never go ahead of it.
3. **Trustworthy.** A published stability policy, a security review, complete docs and a migration guide per tool, provenance on every
   release, and binaries for every supported platform.

Pre-releases use `-alpha.N`, `-beta.N` and `-rc.N`; while the major version is 0, any release may change options, output and plugin
support (see the [changelog](../CHANGELOG.md)).

## Done

- [x] 0.1 Scan the full history and working tree for secrets, stray files and anything that does not belong in a public repository.
- [x] 0.2 Package metadata for `@rathkit/pack` and the platform packages (`repository`, `bugs`, `homepage`, `keywords`, `engines`, `publishConfig`, README, LICENSE in each).
- [x] 0.3 Each promise on the security page backed by a CI check: no runtime dependencies, no install scripts, no network use by default, SBOM with zero components, checksums and provenance on every release.
- [x] 0.4 Document what "alpha" promises and the version policy for 0.x; first release `0.1.0-alpha.0` on the `next` tag, never `latest`.
- [x] 0.7 Release dry run in CI, fix what it finds, publish `0.1.0-alpha.0`, check it with [verifying.md](verifying.md).
- [x] 1.1 A pinned list of open-source projects (Vite apps for React, Vue and Svelte; libraries; command line tools; a webpack or rspack app) built with `rkp` and loaded in a browser, run by `tools/run-corpus.mjs` (by hand, or the weekly `corpus` workflow). Each failure became a fixture and a fix. 24 projects, all passing.
- [x] 1.3 Issue templates, including a compatibility report (tool, plugin, versions), and labels.

Waiting on the maintainer, tracked outside the milestones: 0.5 make the repository public and set up its GitHub settings, 0.6 npm scope, packages and trusted publishing, 0.8 decide what counts as ready to announce.

## Milestones

### 0.1.0-alpha.2

Stable base: required CI on Linux, macOS and Windows; corpus 24 of 24 on every release; zero open security alerts; Firefox and WebKit; options and plugin docs.

- [x] [1.2](https://github.com/rathkit/pack/issues/1) Windows and macOS arm64 legs required in the ruleset, then ten clean runs in a row; the other macOS legs advisory *(maintainer)* `P0`
- [x] [1.4](https://github.com/rathkit/pack/issues/2) Firefox and WebKit for the dev server, HMR and federation scenarios `P1`
- [x] [4.5](https://github.com/rathkit/pack/issues/3) Options reference and plugin docs pages `P1`
- [x] [1.5](https://github.com/rathkit/pack/issues/4) Corpus run on every release and weekly, results kept `P1`
- [x] [1.6](https://github.com/rathkit/pack/issues/5) Explain the flaky tests seen in CI `P1`
- [x] [5.5](https://github.com/rathkit/pack/issues/6) Security alerts at zero and kept there `P1`
- [x] [5.6](https://github.com/rathkit/pack/issues/72) Benchmarks in their own repository, run in a container; benchmark-only tools leave this one `P1`

### 0.2.0-beta.0

Measure and gate: a benchmark suite of 15 workloads with real apps, warm rebuild and HMR measurements, CI gates for size and memory, and a corpus of 60 projects.

- [ ] [3.5](https://github.com/rathkit/pack/issues/7) Benchmark suite of 15 workloads with real apps; warm rebuild, HMR and server start `P0`
- [ ] [3.6](https://github.com/rathkit/pack/issues/8) CI gates: output size and peak memory against a stored baseline `P0`
- [ ] [3.1](https://github.com/rathkit/pack/issues/9) Rebuild and watch-mode benchmarks `P0`
- [ ] [1.8](https://github.com/rathkit/pack/issues/10) Corpus grown to 60 projects `P0`
- [x] [3.14](https://github.com/rathkit/pack/issues/77) Add webpack and Rollup to the benchmark `P1`
- [ ] [3.13](https://github.com/rathkit/pack/issues/79) Benchmark build plus type check, each tool set up the way it is most commonly used `P1`
- [x] [3.15](https://github.com/rathkit/pack/issues/82) Time: minify and source maps without a second parse, so Pack is strictly faster, leaner and smaller than esbuild in every case, a tie counts as a loss `P1`

### 0.3.0-beta.0

CSS and libraries: the CSS pipeline, library mode and legacy targets.

- [ ] [2.1](https://github.com/rathkit/pack/issues/11) CSS pipeline: preprocessors without a config file, PostCSS configuration, CSS modules, @import and url() edge cases `P0`
- [ ] [2.2](https://github.com/rathkit/pack/issues/12) Library mode: dual ESM and CJS, exports maps, preserved modules, --dts-bundle on 30 packages `P0`
- [ ] [2.11](https://github.com/rathkit/pack/issues/76) --dts-bundle and module federation types with TypeScript 7 `P1`
- [ ] [2.4](https://github.com/rathkit/pack/issues/13) --target es5 and a browserslist input `P2`

### 0.4.0-beta.0

Conformance suites: esbuild, webpack, Rollup and Vite, each with its own examples and most-used plugins passing; migration guides.

- [ ] [2.8](https://github.com/rathkit/pack/issues/14) esbuild conformance: every CLI flag and JS API option audited `P0`
- [ ] [2.9](https://github.com/rathkit/pack/issues/15) webpack conformance: configuration options and the 30 most used loaders and plugins `P0`
- [ ] [2.10](https://github.com/rathkit/pack/issues/16) Rollup and Vite conformance: the 50 most used plugins of each `P0`
- [ ] [2.6](https://github.com/rathkit/pack/issues/17) SplitChunksPlugin settings and plugins that read compilation internals `P1`
- [ ] [4.1](https://github.com/rathkit/pack/issues/18) Migration guides from Vite, webpack, esbuild and Rollup `P1`
- [ ] [4.6](https://github.com/rathkit/pack/issues/19) rkp migrate: detect the tool in a project and print or write the equivalent `P2`
- [ ] [4.2](https://github.com/rathkit/pack/issues/20) Bundle analyzer built on --metafile `P2`

### 0.5.0-beta.0

Performance bars met: time, size and memory at or beyond the other benchmarked tools on every workload of the public suite, or listed with a reason.

- [ ] [3.7](https://github.com/rathkit/pack/issues/21) Size: constant folding and inlining across modules `P0`
- [ ] [3.8](https://github.com/rathkit/pack/issues/22) Size: a stronger minifier and frequency-ordered name mangling `P0`
- [ ] [3.9](https://github.com/rathkit/pack/issues/23) Size: CSS minification (shorthand and rule merging) `P1`
- [ ] [3.10](https://github.com/rathkit/pack/issues/24) Time: parallel code generation and per-chunk minify `P1`
- [ ] [3.11](https://github.com/rathkit/pack/issues/25) Time: cheaper source maps `P1`
- [ ] [3.2](https://github.com/rathkit/pack/issues/26) Persistent cache and a faster dev server start `P0`
- [ ] [3.4](https://github.com/rathkit/pack/issues/27) Memory: free per-module data after emit; worst-case inputs `P0`
- [ ] [3.12](https://github.com/rathkit/pack/issues/28) Every workload where another tool wins is listed, with a reason and an issue `P1`

### 0.6.0-beta.0

The Nx plugin: inferred tasks, executors, init, library builds, a repository of its own, tested against Nx versions.

- [ ] [9.1](https://github.com/rathkit/pack/issues/29) Spike: an Nx workspace builds an app and a library with rkp through a custom executor `P0`
- [ ] [9.2](https://github.com/rathkit/pack/issues/30) The rathkit/nx repository and the @rathkit/nx package *(maintainer)* `P0`
- [ ] [9.3](https://github.com/rathkit/pack/issues/31) Nx inferred tasks (createNodesV2): build, serve, watch with inputs, outputs, cache and dependsOn `P0`
- [ ] [9.4](https://github.com/rathkit/pack/issues/32) Nx executors build and serve for project.json `P1`
- [ ] [9.5](https://github.com/rathkit/pack/issues/33) Nx init generator, so nx add @rathkit/nx works `P0`
- [ ] [9.9](https://github.com/rathkit/pack/issues/34) Nx buildable and publishable libraries (library mode, declarations, exports) `P1`
- [ ] [9.10](https://github.com/rathkit/pack/issues/35) Nx version matrix in CI and cache-correctness tests `P0`

### 0.7.0-beta.0

Frameworks: Vite server-side rendering and the Environment API subset; SvelteKit, Astro, Nuxt and React Router build and run; plugin host on older Node; dev server parity.

- [ ] [2.3](https://github.com/rathkit/pack/issues/36) Vite server-side rendering and the Environment API subset frameworks use `P0`
- [ ] [7.1](https://github.com/rathkit/pack/issues/37) SvelteKit builds and runs `P0`
- [ ] [7.2](https://github.com/rathkit/pack/issues/38) Astro builds and runs `P1`
- [ ] [7.3](https://github.com/rathkit/pack/issues/39) Nuxt builds and runs `P1`
- [ ] [7.4](https://github.com/rathkit/pack/issues/40) React Router (Remix) builds and runs `P1`
- [ ] [2.5](https://github.com/rathkit/pack/issues/41) Plugin host on Node versions older than 24 where possible `P1`
- [ ] [1.7](https://github.com/rathkit/pack/issues/42) Dev server and HMR parity checklist: proxy, https, middleware, overlay, framework refresh `P1`

### 0.8.0-beta.0

Tools on top of a bundler, and Nx generators: Storybook, Vitest, Angular; Nx generators, conversions and module federation.

- [ ] [7.5](https://github.com/rathkit/pack/issues/43) Storybook with a Vite or webpack builder runs on rkp `P1`
- [ ] [7.6](https://github.com/rathkit/pack/issues/44) Vitest runs on rkp `P1`
- [ ] [7.7](https://github.com/rathkit/pack/issues/45) Angular (esbuild builder) `P2`
- [ ] [9.6](https://github.com/rathkit/pack/issues/46) Nx configuration generator, application and library generators, and a create-nx-workspace preset `P1`
- [ ] [9.7](https://github.com/rathkit/pack/issues/47) Nx conversion generators from @nx/vite, @nx/webpack, @nx/rspack and @nx/esbuild targets `P0`
- [ ] [9.8](https://github.com/rathkit/pack/issues/48) Nx module federation: hosts and remotes served together, using Nx's generators `P1`

### 0.9.0-beta.0

Rspack and Rsbuild: a spike first, then the Rsbuild configuration and plugin API, Rspack native loaders and plugins, Rslib.

- [ ] [6.1](https://github.com/rathkit/pack/issues/49) Spike: Rsbuild and Rslib examples through a configuration adapter and a plugin shim `P0`
- [ ] [6.2](https://github.com/rathkit/pack/issues/50) Rsbuild configuration and plugin API `P0`
- [ ] [6.3](https://github.com/rathkit/pack/issues/51) Rspack native loaders and plugins: builtin:swc-loader, builtin:lightningcss-loader, CSS extract, copy, define, minimizers `P0`
- [ ] [6.4](https://github.com/rathkit/pack/issues/52) Rslib: library mode on Rsbuild `P1`
- [ ] [6.5](https://github.com/rathkit/pack/issues/53) An @rspack/core-compatible API so an unchanged Rsbuild runs, if the spike says it pays `P2`

### 0.10.0-beta.0

Other bundlers' configurations: tsup, tsdown, unbuild, Rolldown, Parcel, Farm and the Bun.build API shape.

- [ ] [6.6](https://github.com/rathkit/pack/issues/54) tsup, tsdown and unbuild configurations `P1`
- [ ] [6.7](https://github.com/rathkit/pack/issues/55) Rolldown configuration and its options `P1`
- [ ] [6.8](https://github.com/rathkit/pack/issues/56) Parcel: the plugin set and configuration that projects use `P1`
- [ ] [6.9](https://github.com/rathkit/pack/issues/57) Farm configuration `P2`
- [ ] [6.10](https://github.com/rathkit/pack/issues/58) Bun.build API shape `P2`
- [ ] [2.12](https://github.com/rathkit/pack/issues/78) An optional type checker package, the fastest, leanest and smallest to date, with nothing added to the rkp binary `P2`

### 1.0.0-rc.1

Release candidate: stability policy and frozen surface, configuration schema, security review, complete docs, every platform package, two weeks without a new P0.

- [ ] [5.3](https://github.com/rathkit/pack/issues/59) Stability policy for flags, configuration schema and plugin API *(maintainer)* `P0`
- [ ] [4.4](https://github.com/rathkit/pack/issues/60) Configuration schema and editor support `P0`
- [ ] [4.3](https://github.com/rathkit/pack/issues/61) Error and warning message pass `P1`
- [ ] [8.1](https://github.com/rathkit/pack/issues/62) Security review: fuzzing in CI, a threat model of the parser and the plugin host, dependency and workflow audit `P0`
- [ ] [8.2](https://github.com/rathkit/pack/issues/63) Windows arm64 build and npm package *(maintainer)* `P0`
- [ ] [8.3](https://github.com/rathkit/pack/issues/64) Binary signing decision for macOS and Windows *(maintainer)* `P1`
- [ ] [8.4](https://github.com/rathkit/pack/issues/65) Docs complete: every flag, option, plugin kind and error, and a guide per tool `P0`
- [ ] [9.11](https://github.com/rathkit/pack/issues/66) Nx docs page and migration guide `P1`
- [ ] [9.12](https://github.com/rathkit/pack/issues/67) Nx plugin out of alpha and listed in Nx's community plugin registry *(maintainer)* `P1`
- [ ] [8.5](https://github.com/rathkit/pack/issues/68) Release candidate cycle: two weeks with no new P0 `P0`

### 1.0.0

The first stable release.

- [ ] [8.6](https://github.com/rathkit/pack/issues/69) Announcement and the what-is-tested statement *(maintainer)* `P1`
- [ ] [8.7](https://github.com/rathkit/pack/issues/70) 1.0.0 release `P0`

## Project health *(maintainer)*

The licensing model and release cadence are decided by the maintainer and tracked outside this file. The stability policy (5.3),
binary signing (8.3) and the Windows on arm64 package (8.2) are issues above, each needing a maintainer decision or action.
