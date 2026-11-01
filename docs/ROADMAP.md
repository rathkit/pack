# Roadmap

What is planned for Rathkit Pack, in the order it is meant to be done. Each item has a stable ID; once the repository is
public, each ID gets an issue and a milestone (`0.1.0-alpha.0`, `0.1.0-alpha.1`, `0.1.0-beta`). A box is ticked in the
commit that finishes the item. This is a plan, not a promise: the order changes when real projects show what breaks.

Items marked *(maintainer)* need an action on GitHub, npm or elsewhere that only the maintainer can take.

## 0. Public and published

Everything else waits on this: provenance needs a public source repository, and outside feedback needs a release.

- [x] 0.1 Scan the full history and working tree for secrets, stray files and anything that does not belong in a public repository.
- [x] 0.2 Package metadata for `@rathkit/pack` and the platform packages (`repository`, `bugs`, `homepage`, `keywords`, `engines`, `publishConfig`, README, LICENSE in each).
- [x] 0.3 Each promise on the security page backed by a CI check: no runtime dependencies, no install scripts, no network use by default, SBOM with zero components, checksums and provenance on every release.
- [x] 0.4 Document what "alpha" promises and the version policy for 0.x; first release `0.1.0-alpha.0` on the `next` tag, never `latest`.
- [ ] 0.5 Make the repository public and set up its GitHub settings *(maintainer)*.
- [ ] 0.6 npm scope, packages and trusted publishing *(maintainer)*.
- [x] 0.7 Release dry run in CI, fix what it finds, publish `0.1.0-alpha.0`, check it with [verifying.md](verifying.md).
- [ ] 0.8 Decide what counts as ready to announce *(maintainer)*.

Done when: the package installs from npm with provenance, `rkp bundle` works from a clean machine on Linux, macOS and
Windows, and the steps in [verifying.md](verifying.md) pass.

## 1. Learn what breaks on real projects

- [ ] 1.1 A pinned list of 20 or more open-source projects (Vite apps for React, Vue and Svelte; libraries; command line tools; a webpack or rspack app) built with `rkp` and compared with their usual build, run by `tools/run-corpus.mjs`. Each failure becomes a fixture and a fix.
- [ ] 1.2 The full test battery on Windows and macOS in CI, and fewer tests that run on Unix only.
- [ ] 1.3 Issue templates, including a compatibility report (tool, plugin, versions), and labels.
- [ ] 1.4 The development server, `--hmr` and federation scenarios in Firefox and WebKit as well as Chromium.

## 2. Compatibility gaps

In this order, subject to what 1.1 finds.

- [ ] 2.1 CSS: Sass, Less and Stylus through plugins verified, PostCSS configuration, CSS modules checked more widely, `@import` and `url()` edge cases, prefixing and lowering driven by `--target`.
- [ ] 2.2 Library mode: dual ES module and CommonJS output, `exports` maps, preserved modules, `--dts-bundle` checked on 30 or more packages.
- [ ] 2.3 Vite server-side rendering and the parts of its Environment API that frameworks use, `configureServer` breadth, `handleHotUpdate` swapping modules.
- [ ] 2.4 `--target es5` and an optional browserslist input.
- [ ] 2.5 Plugins on Node versions older than 24 where possible, and a longer tested plugin list.
- [ ] 2.6 webpack plugins that use compilation or chunk-graph internals, and `SplitChunksPlugin` settings, only if projects need them.

## 3. Performance in real workflows

- [ ] 3.1 Benchmarks for rebuilds and watch mode against esbuild, rolldown, rspack and Vite (the current ones are cold builds).
- [ ] 3.2 A persistent cache and a faster development server start on large graphs.
- [ ] 3.3 Minified size where others are slightly smaller (the TypeScript compiler, lodash-es).
- [ ] 3.4 Memory and worst-case inputs: very large files and deep graphs.

## 4. Documentation and tooling

- [ ] 4.1 A quick start, migration guides from esbuild, Vite and webpack, and an API reference for the JavaScript API and plugin objects.
- [ ] 4.2 A bundle analyzer built on `--metafile`, and project templates.
- [ ] 4.3 A pass over error and warning messages: source frames, suggestions, links to docs.
- [ ] 4.4 A schema for the configuration file and editor support.

## 5. Project health *(maintainer)*

Licensing model, binary signing, a stability policy for the plugin API and flags before beta, and release cadence are
decided by the maintainer and tracked outside this file.
