# Migration guides

One guide for each tool. Each starts from a small project of that tool (`tests/migrate/<tool>/`: a script that imports a local module,
a CommonJS file, a text file and an aliased path, with a defined constant), shows the tool's own build, then the same build with `rkp`,
and lists what is different. Both builds were run and print the same line when the built program runs.

- [From esbuild](esbuild.md): the command line is nearly the same one; the API only changes its package name.
- [From Vite](vite.md): `rkp bundle index.html`, with the Vite config read for `define` and `resolve.alias`.
- [From webpack](webpack.md): `--config webpack.config.js` is read for the entry, `resolve.alias`, `DefinePlugin` and `module.rules`.
- [From Rollup](rollup.md): the resolve, CommonJS, replace and minify plugins are built in; your own plugins stay in a config file.

The commands of the guides are run by `node tools/run-migrate.mjs`, on every change, against the build under test. The tools' versions
when the guides were written: esbuild 0.28.2, Vite 8.3.5, webpack 5.112.2, Rollup 4.67.5, `rkp` 0.3.0-beta.0. The list of every esbuild
option and what `rkp` does with it is [docs/esbuild-conformance.md](../esbuild-conformance.md).

Things that are the same in every guide:

- `rkp bundle ENTRY... [options]` writes one file (`--outfile`) or, with `--splitting`, a folder (`--outdir`). With neither, the output
  is on the standard output. `rkp serve ENTRY` is the development server (`--hmr` replaces modules in the page without a reload).
- Minifying is off until `--minify`. The output is an ES module until `--format=cjs` or `--format=iife`.
- `--platform=browser` turns `process.env.NODE_ENV` into `"production"` and does not accept Node's built-in modules; the default
  platform is `node`.
- A word that starts with a dash and is not an option is an error, never a file name, so a flag `rkp` does not have stops the build
  instead of being ignored.
