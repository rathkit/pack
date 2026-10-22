# Comparing with other bundlers

`run.mjs` builds the same inputs with rathkit-pack, esbuild, rolldown, rspack and Vite (library mode, which uses rolldown), and
reports for each case the median and best wall time of the whole command (start-up included), peak memory, output size
and gzip size, and whether the output runs and prints what the others print.

```
cargo build --release          # in the repository root
cd bench
npm install                    # the other bundlers and the libraries to bundle, at the versions the results were measured with
node generate.mjs              # the two generated workloads (1,500 JavaScript modules, 500 TypeScript modules)
node run.mjs                   # everything, 7 runs per case: about ten minutes
node run.mjs --markdown --runs 5 lodash react        # chosen workloads, tables as Markdown
```

Options: `--tools rathkit,esbuild`, `--modes plain,minify,minify+map`, `--env production|none` (whether `process.env.NODE_ENV`
is defined as `"production"` in every tool; the default is production). The binary is `target/release/rathkit-pack` unless
`RATHKIT_PACK_BIN` says otherwise. Results are also written to `results.json`.

Workloads: the TypeScript 5.9 compiler (one 9 MB CommonJS file), three.js (a few classes), lodash-es (`export *`), React with
react-dom/server, an rxjs pipeline, and the two generated apps. three.js needs a browser, so it is timed but not run.

For numbers worth quoting: use a machine with nothing else running (`uptime`), a Linux box with `/usr/bin/time`, and look at
the medians. Things to keep in mind when reading them:

- All tools write one ESM file for Node. esbuild, rolldown and Vite cannot run the CommonJS TypeScript compiler as ESM without a
  banner that defines `require`, so their output there is built but reported as "fails".
- Tools pick different package entry points (rxjs has an ESM build and a CommonJS build), which moves output sizes.
- A tool that folds a whole generated app to a constant (rspack does this to `app1500`) makes its size meaningless for that case.
- Time includes Node's start-up for the tools that run on Node.
