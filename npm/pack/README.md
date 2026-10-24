# @rathkit/pack

**Alpha.** Rathkit Pack is a JavaScript and TypeScript bundler written in Rust with no dependencies. This package
starts the program for your platform, from the command line (`rkp`) or from JavaScript. It has no dependencies, runs
nothing when installed, and lists only the platform packages as optional dependencies, so a package manager installs
the one that fits.

```
pnpm add -D @rathkit/pack@next
```

Versions with a suffix such as `0.1.0-alpha.0` are published under the `next` tag, never `latest`.

## Command line

```
pnpm exec rkp bundle src/main.ts --outfile dist/app.mjs --minify
```

or in `package.json`: `"build": "rkp bundle src/main.ts --outfile dist/app.mjs"`. The exit code and the output are the
program's.

## From JavaScript

The same program, started with options in place of flags:

```js
import { build, serve } from '@rathkit/pack';

const result = await build({
  entries: 'src/main.ts',
  outfile: 'dist/app.mjs',
  minify: true,
  sourcemap: true,
  define: { 'process.env.NODE_ENV': '"production"' },
  loader: { '.svg': 'dataurl' },
});
console.log(result.files); // the files written

const server = await serve({ entries: 'src/main.ts', port: 5173 });
console.log(server.url); // http://localhost:5173/
await server.close();
```

- `build(options)` resolves when the program succeeds (with the files written and any warnings) and rejects with a
  `BuildError` otherwise; its `errors` are what the program reported. An option that does not exist is a `TypeError`.
- `serve(options)` starts the development server and resolves once it listens.
- `run(args)` starts the program with command line arguments; a failed run resolves, with its `status`.
- `binaryPath()` and `version()` say which program and which version.
- The options are the command's flags: entries (files, or html pages), `outfile` / `outdir`, `format`, `minify`,
  `sourcemap`, `splitting`, `external`, `alias`, `define`, `loader`, `platform`, `jsx`, `tsconfig`, `config`,
  `federation` (a file, or the options as an object) and the rest. They are typed in `index.d.ts`.
- Plugins are not options: they are listed in a config file (`rathkit.config.mjs`), which the program reads, or whose
  path you give as `config`. The program runs them in a Node process of its own.

Node 24 or later is needed only when a config lists plugins.

What works and what is tested: https://github.com/rathkit/pack#readme. How to check a release:
https://github.com/rathkit/pack/blob/main/docs/verifying.md.

Builds: linux-x64 and linux-arm64 (glibc and musl, so Alpine too), darwin-x64, darwin-arm64, win32-x64. They are not
signed with an operating-system key.
