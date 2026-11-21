# esbuild conformance

Every esbuild command line option and JavaScript API option, with what `rkp` does with it. Each row was run against this build
(`rkp 0.3.0-beta.0`); the probe that ran it is named in the last column, so the table cannot drift: `node tools/run-esbuild-conformance.mjs`
runs all of them (in CI), and a row marked **gap** has a probe that must still fail, so fixing it breaks the test until the row is updated.
The list of options is esbuild's own documentation (its command line help, build, transform and plugin API pages); this machine has no
esbuild to run, so no row compares output byte for byte with esbuild's. What is compared with Node and with esbuild's output is in
[docs/benchmarks.md](benchmarks.md) and the README.

Status:

- **supported**: the option exists, in esbuild's spelling, and does what esbuild's does for the cases the probe covers.
- **partly**: it exists with a difference, stated in the note.
- **not planned**: there is a reason, given.
- **gap**: not done; the number is the draft in the list of larger gaps at the end (each becomes an issue).

How `rkp` is shaped differently, once, so the rows can be short:

- The command is `rkp bundle ENTRY... [options]`. It always bundles: there is no transform-only mode and no `--bundle` switch (`--bundle` and
  `--bundle=true` are accepted and do nothing).
- With no `--outfile` and no `--outdir` the single output goes to the standard output, as in esbuild.
- Options are read as `--name=value`, `--name:key=value` or `--name value`. esbuild's spellings are rewritten before the options are read
  (`src/cli_compat.rs`).
- A word that starts with a dash and is not an option is an error, never a file name. Options a row calls a gap fail this way, so a
  command never builds with an option silently ignored; the exceptions are named as **partly**.
- `--outdir` with one entry and no `--splitting` writes one file named `bundle.js`, and several entries without `--splitting` are one file
  (gap 2). With `--splitting` every entry has its own file named after it, as in esbuild.

## Command line

| Option | Status | Note | Probe |
| --- | --- | --- | --- |
| entry points (several) | partly | one file each only with `--splitting` (ES modules); otherwise one shared file (gap 2) | `entry-points` |
| `--bundle` | supported | accepted; bundling is always on | `bundle` |
| `--outfile=` | supported |  | `outfile` |
| `--outdir=` | partly | the file of a single entry is named `bundle.js` unless `--splitting` (gap 2) | `outdir` |
| `--outbase=` | gap | gap 2 | `gap-outbase` |
| `--allow-overwrite` | supported | a build that would write over one of its own inputs stops, as in esbuild; this flag allows it | `overwrite-refused`, `allow-overwrite` |
| `--format=esm\|cjs\|iife` | supported | also a list for a library, `--format=esm,cjs` | `format-esm`, `format-cjs`, `format-iife` |
| `--format=umd\|amd\|system` | gap | Rollup's formats, not esbuild's; they are an error (they were once built as an ES module without a word) (gap 12) | `gap-format-umd`, `gap-format-amd`, `gap-format-system` |
| `--global-name=` | supported | | `global-name` |
| `--platform=browser\|node` | partly | `node` (the default here) leaves Node's built-in modules as imports; `browser` does not, so an import of `fs` is an error, as in esbuild. With `browser`, `process.env.NODE_ENV` is `"production"` whatever `--minify` says (esbuild: `"development"` unless minifying) (gap 11) | `platform-node`, `platform-browser`, `gap-node-env-development` |
| `--platform=neutral` | partly | treated as `browser`; the default main fields and conditions of esbuild's `neutral` are not applied (gap 11) | `platform-neutral` |
| `--target=` | supported | years and engines, as a list: `es2015,chrome58`; syntax newer than the target is rewritten | `target-list`, `target-esnext` |
| `--supported:feature=` | gap | gap 5 | `gap-supported` |
| `--minify`, `--minify-whitespace`, `--minify-identifiers`, `--minify-syntax` | supported | output size is compared with esbuild in the benchmark | `minify`, `minify-whitespace`, `minify-identifiers`, `minify-syntax` |
| `--keep-names` | supported | | `keep-names` |
| `--mangle-props=`, `--reserve-props=`, `--mangle-quoted`, `--mangle-cache=` | gap | gap 4 | `gap-mangle-props`, `gap-reserve-props`, `gap-mangle-quoted`, `gap-mangle-cache` |
| `--line-limit=` | gap | gap 10 | `gap-line-limit` |
| `--charset=utf8` | supported | non-ASCII text is always kept as written | `charset-utf8` |
| `--charset=ascii` (esbuild's default) | gap | esbuild escapes non-ASCII text by default; this program does not (gap 10) | `gap-charset-ascii` |
| `--sourcemap`, `--sourcemap=linked\|inline\|external` | supported | | `sourcemap-linked`, `sourcemap-inline`, `sourcemap-external` |
| `--sourcemap=both` | gap | gap 8 | `gap-sourcemap-both` |
| `--sources-content=false` | supported | | `sources-content` |
| `--source-root=`, `--sourcefile=` | gap | gap 8 | `gap-source-root`, `gap-sourcefile` |
| `--splitting` | supported | ES modules only, as in esbuild | `splitting` |
| `--chunk-names=`, `--entry-names=` | partly | `[name]` and `[hash]`; no folders (`[dir]`, a `/` in the template) (gap 3) | `chunk-names`, `entry-names`, `gap-chunk-names-folder` |
| `--asset-names=` | supported | `[name]`, `[hash]`, `[ext]` | `asset-names` |
| `--out-extension:.js=.mjs` | supported | also `.css` | `out-extension` |
| `--public-path=` | supported | | `public-path` |
| `--define:K=V` | supported | | `define` |
| `--external:NAME` | supported | a name also covers what is inside the package (`react/jsx-runtime`); one `*` stands for any text (`@scope/*`, `*.png`) | `external-name`, `external-wildcard`, `external-package-subpath` |
| `--packages=external\|bundle` | supported | | `packages` |
| `--alias:A=B` | supported | | `alias` |
| `--loader:.ext=` | partly | `text`, `base64`, `dataurl`, `binary`, `file`, `empty`, `css`, `local-css`; not `js`, `jsx`, `ts`, `tsx`, `json`, `copy`, `global-css`, `default` (gap 1) | `loader-text`, `loader-json-dataurl-base64-binary-file-empty-css`, `gap-loader-ts`, `gap-copy-loader`, `gap-global-css-loader` |
| `--inject:FILE` | supported | | `inject` |
| `--banner:js=`, `--banner:css=`, `--footer:js=`, `--footer:css=` | supported | | `banner`, `footer` |
| `--drop:console`, `--drop:debugger` | supported | | `drop-console-debugger` |
| `--drop-labels=` | gap | gap 5 | `gap-drop-labels` |
| `--pure:NAME` | supported | | `pure` |
| `--ignore-annotations` | gap | gap 5 | `gap-ignore-annotations` |
| `--tree-shaking=true\|false` | supported | | `tree-shaking-false` |
| `--legal-comments=none\|eof\|linked\|external` | supported | not `inline` | `legal-comments` |
| `--jsx=automatic\|transform` | supported | `transform` is `--jsx classic` here | `jsx-automatic`, `jsx-transform` |
| `--jsx=preserve` | gap | gap 5 | `gap-jsx-preserve` |
| `--jsx-factory=`, `--jsx-fragment=`, `--jsx-import-source=`, `--jsx-dev` | supported | | `jsx-transform`, `jsx-fragment`, `jsx-import-source`, `jsx-dev` |
| `--jsx-side-effects` | gap | gap 5 | `gap-jsx-side-effects` |
| `--tsconfig=`, `--tsconfig-raw=` | supported | | `tsconfig-file`, `tsconfig-raw` |
| `--main-fields=` | supported | | `main-fields` |
| `--conditions=` | supported | | `conditions` |
| `--resolve-extensions=` | supported | | `resolve-extensions` |
| `--preserve-symlinks` | supported | | `preserve-symlinks` |
| `--metafile=` | supported | the shape of esbuild's metafile (`outputs` are named from the working folder and list every file written, each with a `kind`, which esbuild's does not have); `bytesInOutput` is an estimate; `rkp analyze` makes a report of it | `metafile`, and `tests/analyze.rs` |
| `--analyze` | gap | gap 10 (`rkp analyze meta.json` is the way to read a metafile) | `gap-analyze` |
| `--abs-paths=` | gap | gap 10 | `gap-abs-paths` |
| `--log-level=`, `--color=`, `--log-limit=` | partly | accepted and ignored: messages go to the standard error | `log-level`, `color`, `log-limit` |
| `--log-override:` | gap | gap 10 | `gap-log-override` |
| `--watch` | supported | the `--watch=forever` form is a gap (gap 9) | `tools/run-watch.mjs` |
| `--serve=`, `--servedir=`, `--serve-fallback=`, `--certfile=`, `--keyfile=` | gap | `rkp serve ENTRY` is a different command with its own options (`--port`, `--host`, `--https`, `--https-cert`, `--https-key`, `--cors-origin`, `--hmr`); gap 9 | `gap-servedir`, `gap-serve-fallback`, `gap-certfile`, `gap-keyfile`, `tools/run-serve.mjs` |
| standard input as the entry (`--stdin`, `--sourcefile` with it) | gap | gap 6 | `gap-sourcefile` |
| `--version` | supported | prints `rkp VERSION` | `tools/test-npm-wrapper.mjs` |
| `--help` | not planned | the usage line is printed when no command is given; the options are in the README | none |

## JavaScript API: `build`

`@rathkit/pack` has `build(options)` and `serve(options)`; the option names are mostly the same as esbuild's, and the ones that
differ are listed. An option the API does not have is a `TypeError` (a typo is never ignored); esbuild's options that are gaps fail that way.

| Option | Status | Note | Probe |
| --- | --- | --- | --- |
| `entryPoints` | partly | `entries` is the name; `entryPoints` is accepted as file names; esbuild's object form and `{ in, out }` are not (gap 2) | `api-entryPoints`, `gap-api-entryPoints-object` |
| `absWorkingDir` | supported | `cwd` is the name; this one is accepted | `api-absWorkingDir` |
| `bundle` | supported | accepted when `true`; `false` is a `TypeError` | `api-bundle-true`, `gap-api-bundle-false` |
| `outfile`, `outdir`, `format`, `globalName`, `platform`, `target`, `minify` (a boolean, or `{ whitespace, identifiers, syntax }`), `keepNames`, `sourcemap`, `sourcesContent`, `splitting`, `treeShaking`, `define`, `external`, `packages`, `alias`, `loader`, `inject`, `banner`, `footer`, `drop`, `pure`, `legalComments`, `entryNames`, `chunkNames`, `assetNames`, `outExtension`, `publicPath`, `jsx`, `jsxDev`, `jsxFactory`, `jsxFragment`, `jsxImportSource`, `tsconfig`, `tsconfigRaw`, `mainFields`, `conditions`, `resolveExtensions`, `preserveSymlinks` | supported | the same as the command line option of the rows above, with the notes there | `api-banner-footer`, and the command line probes |
| `allowOverwrite` | supported | | `api-allowOverwrite` |
| `charset` | supported | only `'utf8'` | `api-charset-utf8`, `gap-charset-ascii` |
| `logLevel`, `color`, `logLimit` | partly | accepted and ignored; messages are in the result's `warnings` and `errors` | `api-logLevel` |
| `metafile` | partly | a file name, not `true`; the result has the metafile too (esbuild: `true`, and the result has it) (gap 7) | `gap-api-metafile-boolean` |
| `write` | gap | `write: false` and `outputFiles` (gap 7) | `gap-api-write-false` |
| `stdin` | gap | gap 6 | `gap-api-stdin` |
| `supported`, `dropLabels`, `ignoreAnnotations`, `jsxSideEffects` | gap | gap 5 | `gap-api-supported` |
| `mangleProps`, `reserveProps`, `mangleQuoted`, `mangleCache` | gap | gap 4 | `gap-api-mangleProps` |
| `outbase`, `sourceRoot`, `lineLimit`, `logOverride`, `nodePaths` | gap | gaps 3, 8, 10 | none (an unknown option is a `TypeError`) |
| `plugins` | partly | plugin objects (esbuild's `{ name, setup(build) }` among them, and Rollup and Vite hooks), `{ module }` descriptors, or a config file; see the plugin table | `tools/run-plugins.mjs` |

Results: `build` resolves with `{ ok, status, files, metafile, warnings, errors, stdout, stderr }` (`files` is every file written, as absolute paths; esbuild's is `{ errors, warnings,
outputFiles, metafile, mangleCache }`; the messages here are strings, not objects with a `location`) and rejects with a `BuildError` when
the build fails (esbuild rejects the same way).

## JavaScript API: functions

| Function | Status | Note | Probe |
| --- | --- | --- | --- |
| `build` | supported | above | `api-*` |
| `buildSync` | not planned | the build is a child process; a synchronous call would block the event loop for the whole build, and `rkp bundle` is the synchronous form | `gap-api-buildSync` |
| `transform`, `transformSync` | gap | gap 6 | `gap-api-transform`, `gap-api-transformSync` |
| `context` with `rebuild`, `watch`, `serve`, `cancel`, `dispose` | gap | the command has `--watch` and `rkp serve`, and the API has `serve(options)`; there is no context object (gap 7) | `gap-api-context`, `tools/run-watch.mjs`, `tools/run-serve.mjs` |
| `serve` | partly | not esbuild's `ctx.serve()`: `serve(options)` starts `rkp serve` and resolves with `{ url, close() }` | `tools/test-plugin-objects.mjs` |
| `formatMessages`, `analyzeMetafile`, `analyzeMetafileSync` | gap | `rkp analyze` makes an HTML report of a metafile; the text form is gap 10 | `gap-api-analyzeMetafile`, `gap-api-analyzeMetafileSync`, `gap-api-formatMessages` |
| `version` | supported | `version()` | `tools/test-npm-wrapper.mjs` |
| `initialize` (WebAssembly build) | not planned | there is no WebAssembly build; the package ships the native program | none |

## Plugin API (`setup(build)`)

esbuild plugins are accepted from a config file (`rathkit.config.mjs`, `--config FILE`) and, with the object form, from the `plugins`
option. Their effects are checked with small plugins in `tests/plugins/esbuild-plugin` (`tools/run-plugins.mjs`) and the probes named below.

| Feature | Status | Note | Probe |
| --- | --- | --- | --- |
| `setup(build)` | supported | | `tools/run-plugins.mjs` (esbuild-plugin) |
| `build.initialOptions` | partly | `entryPoints`, `bundle`, `outdir`, `outfile`, `format` (`esm`, `cjs`, `iife`), `platform`, `minify*`, `sourcemap`, `define`, `external`, `loader`, `splitting`, `absWorkingDir`, `write`, `metafile` are set from the command line (`metafile` is always `false`); the rest are missing (gap 11) | `plugin-initial-options` |
| `build.esbuild` | partly | an object with a `version` | `plugin-initial-options` |
| `build.onStart`, `build.onEnd`, `build.onDispose` | partly | they run; `onEnd`'s result has empty `errors` and `warnings` and no `metafile` or `outputFiles` (gap 11) | `tools/run-plugins.mjs`, `gap-plugin-onend-metafile` |
| `build.onResolve({ filter, namespace })` | supported | arguments `path`, `importer`, `namespace`, `resolveDir`, `kind`, `pluginData`; results `path`, `namespace`, `external`, `pluginData`, `sideEffects`, `errors`, `warnings` | `tools/run-plugins.mjs` |
| `build.onLoad({ filter, namespace })` | partly | `contents`, `loader`, `resolveDir`, `pluginData`, `errors`, `warnings` work; `suffix` is always empty, `with` is always `{}`, `watchFiles` and `watchDirs` are ignored (gap 11) | `tools/run-plugins.mjs` |
| `build.resolve()` | supported | `suffix` is always empty (gap 11) | manual check in this audit; `tools/run-plugins.mjs` covers `onResolve` |
| `build.context` | gap | gap 11 | none |
| `kind` values `entry-point`, `import-statement`, `require-call`, `dynamic-import`, `require-resolve` | supported | `import-rule` and `url-token` (CSS) are not passed | `tools/run-plugins.mjs` |

## Larger gaps

Each is a candidate for an issue. They are numbered as the rows above name them.

1. `--loader` with `js`, `jsx`, `ts`, `tsx`, `json`, `copy`, `global-css`, `default`.
2. Output naming: one file per entry without `--splitting`, a single entry's file named after it, `--outbase`, `entryPoints` objects.
3. `[dir]` and folders in `--entry-names` and `--chunk-names`.
4. Property mangling: `--mangle-props`, `--reserve-props`, `--mangle-quoted`, `--mangle-cache`.
5. Feature switches: `--supported:`, `--drop-labels`, `--ignore-annotations`, `--jsx-side-effects`, `--jsx=preserve`.
6. A transform API and standard input as the entry.
7. A context API (`rebuild`, `watch`, `serve`, `cancel`, `dispose`), `write: false` with `outputFiles`, `metafile: true`.
8. Source map extras: `--sourcemap=both`, `--source-root`, `--sourcefile`.
9. esbuild's `--serve`, `--servedir`, `--serve-fallback`, `--certfile`, `--keyfile`, `--watch=forever`.
10. Output and message options: `--charset=ascii`, `--line-limit`, `--abs-paths`, `--log-override`, `--analyze`, `analyzeMetafile`, `formatMessages`.
11. Plugin API completeness and `--platform=neutral` defaults.
12. Output formats `umd`, `amd` and `system` (Rollup's `output.format`; esbuild has none of them).
