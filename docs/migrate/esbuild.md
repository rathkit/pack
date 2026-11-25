# From esbuild

`rkp bundle` reads esbuild's spellings: `--outfile=FILE`, `--external:NAME`, `--alias:A=B`, `--define:K=V`, `--loader:.ext=NAME`,
`--platform=browser`, `--target=es2020`, `--banner:js=TEXT`, and the rest listed in [the conformance table](../esbuild-conformance.md),
which says for every esbuild option whether it is supported, partly, not planned or not done yet.

## Command line

Before (`package.json`; esbuild 0.28.2):

```
esbuild src/main.js --bundle --outfile=dist/main.js --format=esm --minify --sourcemap --platform=browser --define:__APP_VERSION__='"1.2.3"' --loader:.txt=text --alias:@=./src
```

After (`rkp` 0.3.0-beta.0):

```
rkp bundle src/main.js --bundle --outfile=dist/main.js --format=esm --minify --sourcemap --platform=browser --define:__APP_VERSION__='"1.2.3"' --loader:.txt=text --alias:@=./src
```

`--bundle` is accepted and does nothing (`rkp` always bundles). Both builds print `hello world 42 aliased from a text file 1.2.3 production`
when the program runs; the esbuild file is 787 bytes and the `rkp` file 361, on this one small program (not a benchmark: those are in
[docs/benchmarks.md](../benchmarks.md)).

## JavaScript API

Before:

```js
import { build } from 'esbuild';

await build({
  entryPoints: ['src/main.js'],
  bundle: true,
  outfile: 'dist/main.js',
  format: 'esm',
  platform: 'browser',
  minify: true,
  sourcemap: true,
  define: { __APP_VERSION__: '"1.2.3"', 'process.env.NODE_ENV': '"production"' },
  loader: { '.txt': 'text' },
  alias: { '@': './src' },
});
```

After: the package name.

```js
import { build } from '@rathkit/pack';
```

The options are the same names (`entryPoints` as file names, `absWorkingDir`, `bundle: true`, `charset: 'utf8'` and the log options are
accepted next to the package's own `entries` and `cwd`). `build` resolves with `{ ok, status, files, metafile, warnings, errors, ... }`
and rejects with a `BuildError` when the build fails, as esbuild's does.

`rkp migrate` finds the scripts that use the API (a file of the root, or one that a package.json script runs, such as `node scripts/build.mjs`) and says, line by line, what changes: the line that imports `esbuild` and what it becomes, the lines of the `build()` calls (the same call), and the lines of everything the package does not have (`context`, `transform`, `buildSync`, `write: false`, `stdin`, `mangleProps`, ...), each with the reason; a plugin imported from `esbuild-plugin-*` stays in `plugins`. It does not rewrite the script: the script is yours, and the change is one line.

When the entry points and the `outfile` or `outdir` and `format` of the `build()` calls are plain strings (also when a helper adds the same options to each call), the commands are printed too, one for each call whose format `rkp` can write; a umd build gets a note instead.

## What is different

- **Several entries.** Without `--splitting` they become one file, and with `--outdir` and one entry the file is named `bundle.js`, not
  after the entry. With `--splitting` (ES modules) each entry has its own file. `--outbase` and `entryPoints` as an object are not done.
- **`--charset`.** esbuild escapes non-ASCII text by default; `rkp` keeps it as written (`--charset=utf8`); `--charset=ascii` is an error.
- **`--loader`.** `text`, `base64`, `dataurl`, `binary`, `file`, `empty`, `css` and `local-css`. Not `js`, `jsx`, `ts`, `tsx`, `json`,
  `copy` (files with those extensions are read by what they are called anyway).
- **`transform`, `context`, `write: false`, `stdin`.** Not there: there is no transform-only mode or context object. `--watch` and
  `rkp serve` are the command line's forms.
- **`--serve` and `--servedir`.** `rkp serve ENTRY [--port N] [--host H] [--hmr]` is a different command.
- **`--mangle-props`, `--supported:`, `--drop-labels`, `--jsx=preserve`.** Not done; the build stops with an error.
- **`--external`.** A name also covers what is inside the package (`react` and `react/jsx-runtime`); one `*` stands for any text.
- **`--metafile`.** The same shape; `rkp analyze meta.json --outfile report.html` makes a report of it.

## Plugins

esbuild plugins (`setup(build)` with `onResolve`, `onLoad`, `onStart`, `onEnd`) work from `rathkit.config.mjs` or `--config FILE`, and
as objects in the API's `plugins` option; see the plugin table of the conformance document for what each hook passes.
