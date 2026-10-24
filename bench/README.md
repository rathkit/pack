# Measuring

`run.mjs` builds fixed inputs with rathkit-pack (and, with `--tools`, esbuild, rolldown, rspack and Vite) and reports for each case the median and best wall time of the whole
command (start-up included), peak memory, output size and gzip size, and whether the output runs and prints what the
unbundled program prints. Figures are quoted only with this method and the machine they were measured on; the published ones are in [../docs/benchmarks.md](../docs/benchmarks.md).

```
cargo build --release          # in the repository root
cd bench
npm install                    # the libraries to bundle and the packages the tests use, at pinned versions
node generate.mjs              # the two generated workloads (1,500 JavaScript modules, 500 TypeScript modules)
node run.mjs                   # everything, 7 runs per case: about ten minutes
node run.mjs --markdown --runs 5 lodash react        # chosen workloads, tables as Markdown
```

Options: `--tools rathkit,esbuild,rolldown,rspack,vite` (default: rathkit only), `--modes plain,minify,minify+map`, `--env production|none` (whether `process.env.NODE_ENV`
is defined as `"production"`; the default is production). The binary is `target/release/rathkit-pack` unless
`RATHKIT_PACK_BIN` says otherwise. Results are also written to `results.json`.

Workloads: the TypeScript 5.9 compiler (one 9 MB CommonJS file), three.js (a few classes), lodash-es (`export *`), React with
react-dom/server, an rxjs pipeline, and the two generated apps. three.js needs a browser, so it is timed but not run.

For numbers worth quoting: use a machine with nothing else running (`uptime`), a Linux box with `/usr/bin/time`, and look at
the medians. Things to keep in mind when reading them:

- The output is one ESM file for Node.
- Time includes the process start-up.
- Output sizes depend on which package entry point is picked (rxjs has an ESM build and a CommonJS build).
