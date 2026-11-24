# Benchmarks

Measured, with the method below, on one machine. Other machines, other versions and other inputs give other numbers;
run the container recipe in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) to get your own. Nothing here is a claim about inputs that were not measured.

## Method

- **What is timed:** the whole command from start to exit, start-up included (`/usr/bin/time`), after one warm-up run;
  the median of 7 runs is shown. Peak memory is the largest resident size of the process.
- **What is compared:** the same entry, producing one ES module file for Node, with `process.env.NODE_ENV` defined as
  `"production"` in every tool, in three modes: plain, minified, and minified with a source map.
- **Do the outputs work:** each output that can run is run, and what it prints is compared with the first tool's output
  ("same" / "fails" / "DIFFERENT"; a program that prints nothing counts as failed). esbuild is run with `--platform=node` and is
  given a banner that defines `require`, `__filename` and `__dirname`, so its ES module output can run the CommonJS TypeScript
  compiler and React. Where another bundler "fails" the same way (rolldown, Vite, Rollup and Parcel 2 on the TypeScript compiler,
  Vite on React; Rollup's Solid output differs) or does not finish (Parcel 2 on the 1,500-module app: an internal assertion error
  in all three modes), it is not counted against the speed.
- **Parcel:** Parcel 2 and Parcel 3 are built from the `targets` in pack-bench's `package.json` (Node, ES module, node_modules
  included). Parcel 2 is run without its disk cache. Parcel 3 is run as its native executable, so its time has no Node
  start-up in it. Without `--optimize` Parcel does not minify and does not scope-hoist, so its plain build is not the same
  kind of build as the others.
- **webpack and Rollup:** webpack runs in production mode with `swc-loader` for TypeScript and its own default minimizer
  (terser). Rollup runs with node-resolve, commonjs, replace (the `NODE_ENV` define), `@rollup/plugin-typescript` and, for
  the minified cases, `@rollup/plugin-terser`, because Rollup has no minifier of its own, so their minified times partly
  measure terser. Neither type-checks (`swc-loader` only strips types; the Rollup plugin runs with `noCheck`), and neither
  does any other tool in the table.
- **Other tools** are used with the options listed in pack-bench's `run.mjs` (for rspack, a config with `builtin:swc-loader` for
  TypeScript; for Vite, library mode), not tuned beyond that. A better configuration of any of them may do better.
- **Workloads (15):** the TypeScript 5.9 compiler (one 9 MB CommonJS file), three.js (a few classes), lodash-es (`export *`),
  React with react-dom/server, an rxjs pipeline, a Preact app (TSX, hooks and context), a Vue 3 app (render functions, so no
  compiler plugin), a Solid app (no JSX, so no compiler plugin), a Svelte 5 app (its components compiled to server code by Svelte's
  own compiler when the container image is built, so the compiler's time is not measured), a monorepo-style TypeScript app (four
  workspace packages linked into `node_modules/@mono` that name their TypeScript source in `exports`, and one app), date-fns, Zod,
  D3, and two generated apps (1,500 JavaScript modules and 500 TypeScript modules, pack-bench's `generate.mjs`). The framework
  apps were written for pack-bench, small programs that use the framework as an application does; they are not open-source
  applications taken from elsewhere, and the Vue, Solid and Svelte ones are server-rendered programs that the tools bundle for
  Node. three.js and lodash are timed but not run. Pack and Vite are given the `node` export condition for the Solid app (the other
  tools add it for a Node build). Parcel 2 is run with its `packageExports` resolver option on, because without it it cannot
  resolve packages that only have `exports`.
- **Warm rebuild:** each tool is run in its own watch mode on a writable copy of the project, in plain and in minified mode. The
  first build is waited for, then one project file is changed 7 times and the time from writing the file to the output holding
  the change is taken (the output file is polled every 2 ms); the median is shown. Each tool uses its default watcher. Pack hears
  of a change through the file events of the kernel on Linux (inotify) and starts the build as soon as the writer has closed the
  file, or after 4 ms without news; where there are no events (macOS, Windows) it polls the files its build read every 80 ms and
  waits 40 ms more for writes to settle, which is up to 120 ms of its figure. Pack keeps the modules of the last build whose files
  did not change, the scan of a module that imports nothing from the bundle, and the pieces a big bundle is minified in, so a warm
  rebuild does the work that the change needs. esbuild scans for changes in rounds, so its times vary more (the minimum and the
  median are in the results file); the others use the operating system's file events. Parcel 2 builds in development mode when it watches and has no minified watch mode.
- **Server start and HMR latency:** for the tools that have a development server (Pack `serve --hmr`, esbuild `--serve`, Vite,
  webpack with webpack-dev-server, rspack `serve`, Parcel 2 and 3 `serve`); rolldown and Rollup have none. No browser is used. A
  page load is imitated by asking for the page, the scripts it names and (for Vite, which does not bundle) every module they
  import, six requests at a time; the server has started when all are answered. HMR latency is from writing a change to a module
  that already accepts its own update to the update notice on the tool's own event channel (a WebSocket, or server-sent events);
  what the page then does with it is not measured, and the notices differ in kind (the table says which). Pack's notice is the
  update itself: it holds the code of each module that changed, and it is sent as soon as the modules are written, before the build's
  files (the source map is the longest part) are finished. esbuild's `change` notice only says that a rebuild finished; the page then
  fetches the output to get the change, which is not in the figure. So the cells compare an update that a page can apply with a
  signal that it has to ask for. The TypeScript compiler is
  not served: it is a library for Node with no page, it needs `fs`, `path`, `os` and other Node modules, and each tool would have to be
  set up to stub them in its own way, so its cells say n/a. Parcel's development copy gets the same `packageExports` resolver option the
  build uses. webpack and rspack leave `node:` imports as empty externals (webpack cannot read them for a browser target, and Svelte's
  server runtime names one), and their `warnings` notice, which they send instead of `ok` when a rebuild has warnings, counts as the
  update, because the development server's client applies the update after either.
- **Build plus type check (a separate table, TypeScript workloads only):** each tool is set up the way it is commonly used: a separate
  `tsc --noEmit` (esbuild, rolldown, Parcel 2 and 3; Vite's starter template runs `tsc -b` before `vite build`), the checker plugin
  of its own bundler (`ts-checker-rspack-plugin` for rspack, `fork-ts-checker-webpack-plugin` for webpack, both with `async: false`,
  their default in a production build, so the build waits for the check), or `@rollup/plugin-typescript` with checking on (its
  default; it reports type errors as warnings). The build is the minified one. Wall time is from the start of the first process to the
  exit of the last; peak memory is the largest sum of the resident sizes of every process of the setup at one moment (sampled every
  10 ms, and never less than the kernel's peak for the largest process), so it counts shared pages once per process and is an upper
  bound. The build-only column uses the same inputs and the project's own tsconfig, so the difference is the cost of the check.
  Rathkit Pack's own checker (`rkp check`, stage 1) is not measured here, so its row has no figure. Each plugin setup was first tried on a deliberate type error to
  confirm that it reports it.
<!-- pack-bench:begin -->
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.58.1-microsoft-standard-WSL2; WSL2, Docker Desktop 29.9.3, laptop on AC. The machine was idle: the load average was 0.52, 1.01, 1.25 before the run and 1.11, 1.26, 1.38 after it. Node 26.13.0; Rathkit Pack 0.4.0-beta.0 at commit `8931949` (release build, sha256 `e76397fd0482`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha), webpack-dev-server 6.1.0, @rspack/dev-server 2.2.2, TypeScript 5.9.3 (tsc), ts-checker-rspack-plugin 1.7.2, fork-ts-checker-webpack-plugin 9.1.0. Run on 2026-11-24 in the pack-bench container, bench commit `f1b5813`, image `1535fd26ad5d`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `f1b5813`, build Pack at commit `8931949` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261124T042236Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --rebuild --rebuild-modes plain,minify --dev --typecheck`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 181 | 368 | 711 | 345 | 886 | 1290 | 8792 | 2319 | 579 |
| three.js | 42 | 61 | 138 | 149 | 240 | 600 | 2468 | 733 | 84 |
| lodash-es | 20 | 36 | 115 | 126 | 187 | 760 | 3646 | 1093 | 9 |
| React + react-dom/server | 13 | 20 | 109 | 99 | 176 | 397 | 2221 | 596 | 40 |
| rxjs | 12 | 23 | 110 | 107 | 168 | 573 | 2926 | 768 | 25 |
| 1,500-module app | 69 | 493 | 220 | 582 | 310 | 1870 | 5612 | failed | 198 |
| 500-module TypeScript app | 24 | 57 | 146 | 332 | 226 | 957 | 2258 | 1228 | 72 |
| Preact app (TSX) | 5 | 11 | 90 | 95 | 154 | 347 | 1932 | 558 | 15 |
| Vue app (render functions) | 18 | 44 | 152 | 125 | 184 | 634 | 2226 | 684 | 60 |
| Solid app (no JSX) | 9 | 16 | 92 | 102 | 162 | 371 | 1895 | 563 | 22 |
| Svelte app (precompiled) | 10 | 16 | 101 | 110 | 171 | 480 | 2267 | 616 | 22 |
| Monorepo-style TypeScript app | 13 | 22 | 97 | 145 | 167 | 656 | 2044 | 845 | 27 |
| date-fns | 13 | 29 | 103 | 115 | 176 | 493 | 4346 | 787 | 35 |
| Zod | 13 | 19 | 106 | 111 | 177 | 539 | 2352 | 633 | 30 |
| D3 | 15 | 24 | 111 | 104 | 180 | 509 | 2218 | 600 | 30 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 267 | 375 | 834 | 1766 | 1040 | 18886 | 28503 | 3748 | 1433 |
| three.js | 47 | 70 | 154 | 306 | 250 | 2543 | 3637 | 877 | 135 |
| lodash-es | 23 | 37 | 121 | 163 | 195 | 1497 | 4170 | 1138 | 9 |
| React + react-dom/server | 17 | 20 | 114 | 175 | 183 | 1479 | 2983 | 794 | 67 |
| rxjs | 14 | 23 | 115 | 158 | 168 | 1475 | 3195 | 815 | 28 |
| 1,500-module app | 73 | 499 | 221 | 787 | 316 | 4114 | 5650 | failed | 185 |
| 500-module TypeScript app | 28 | 56 | 146 | 486 | 232 | 3912 | 3030 | 1292 | 81 |
| Preact app (TSX) | 7 | 10 | 89 | 112 | 158 | 645 | 2131 | 596 | 17 |
| Vue app (render functions) | 22 | 47 | 171 | 357 | 197 | 3297 | 2806 | 930 | 95 |
| Solid app (no JSX) | 12 | 17 | 95 | 127 | 167 | 914 | 2026 | 589 | 32 |
| Svelte app (precompiled) | 13 | 16 | 105 | 131 | 176 | 962 | 2489 | 656 | 26 |
| Monorepo-style TypeScript app | 16 | 22 | 99 | 237 | 169 | 2634 | 2343 | 840 | 27 |
| date-fns | 15 | 29 | 102 | 129 | 172 | 784 | 4583 | 811 | 36 |
| Zod | 16 | 20 | 109 | 149 | 177 | 1286 | 2863 | 697 | 36 |
| D3 | 18 | 25 | 115 | 152 | 185 | 1319 | 2716 | 677 | 50 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 329 | 537 | 945 | 2011 | 1144 | 19572 | 29563 | 4841 | 1535 |
| three.js | 59 | 87 | 162 | 355 | 254 | 2730 | 3761 | 1086 | 148 |
| lodash-es | 29 | 43 | 126 | 181 | 195 | 1571 | 4184 | 1192 | 8 |
| React + react-dom/server | 23 | 26 | 119 | 193 | 190 | 1559 | 3071 | 887 | 73 |
| rxjs | 17 | 30 | 121 | 169 | 173 | 1532 | 3201 | 848 | 30 |
| 1,500-module app | 86 | 506 | 225 | 820 | 321 | 4214 | 5643 | failed | 189 |
| 500-module TypeScript app | 35 | 61 | 149 | 534 | 232 | 3959 | 3106 | 1460 | 79 |
| Preact app (TSX) | 9 | 13 | 92 | 113 | 158 | 673 | 2154 | 602 | 19 |
| Vue app (render functions) | 26 | 92 | 192 | 422 | 212 | 3505 | 3022 | 1085 | 102 |
| Solid app (no JSX) | 14 | 18 | 99 | 132 | 169 | 967 | 2036 | 619 | 36 |
| Svelte app (precompiled) | 14 | 18 | 102 | 139 | 168 | 1005 | 2514 | 688 | 28 |
| Monorepo-style TypeScript app | 18 | 24 | 102 | 262 | 171 | 2708 | 2382 | 926 | 29 |
| date-fns | 17 | 30 | 102 | 132 | 177 | 820 | 4598 | 843 | 37 |
| Zod | 21 | 22 | 114 | 155 | 184 | 1328 | 2888 | 721 | 44 |
| D3 | 21 | 26 | 119 | 161 | 192 | 1378 | 2745 | 727 | 51 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 2.0x, rolldown 7.1x, rspack 8.0x, Vite 11.2x, webpack 33.6x, Rollup 151.9x, Parcel 2 46.6x, Parcel 3 2.3x;
minified, esbuild 1.7x, rolldown 6.0x, rspack 10.4x, Vite 9.4x, webpack 83.5x, Rollup 154.9x, Parcel 2 42.5x, Parcel 3 2.4x;
minified with a map, esbuild 1.6x, rolldown 5.2x, rspack 9.3x, Vite 8.0x, webpack 72.0x, Rollup 130.4x, Parcel 2 38.5x, Parcel 3 2.1x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 191 | 265 | 499 | 422 | 470 | 1962 | 3162 | 1391 | 373 |
| three.js | 42 | 60 | 205 | 191 | 230 | 489 | 710 | 566 | 141 |
| lodash-es | 21 | 56 | 235 | 145 | 260 | 351 | 567 | 533 | 27 |
| React + react-dom/server | 21 | 29 | 203 | 131 | 213 | 326 | 595 | 504 | 186 |
| rxjs | 12 | 49 | 222 | 130 | 204 | 339 | 486 | 488 | 88 |
| 1,500-module app | 95 | 380 | 468 | 352 | 491 | 1038 | 844 | failed | 246 |
| 500-module TypeScript app | 27 | 55 | 192 | 178 | 203 | 535 | 597 | 571 | 81 |
| Preact app (TSX) | 9 | 20 | 143 | 113 | 169 | 211 | 436 | 346 | 58 |
| Vue app (render functions) | 18 | 88 | 323 | 175 | 208 | 541 | 622 | 587 | 197 |
| Solid app (no JSX) | 12 | 22 | 163 | 119 | 191 | 241 | 413 | 381 | 81 |
| Svelte app (precompiled) | 11 | 27 | 204 | 122 | 220 | 234 | 466 | 421 | 90 |
| Monorepo-style TypeScript app | 14 | 34 | 168 | 155 | 188 | 464 | 508 | 526 | 71 |
| date-fns | 14 | 41 | 196 | 123 | 224 | 206 | 767 | 517 | 77 |
| Zod | 20 | 42 | 260 | 132 | 278 | 317 | 549 | 448 | 139 |
| D3 | 20 | 41 | 266 | 132 | 295 | 302 | 553 | 453 | 152 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 3466 / 975 | 3515 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 | 3467 / 959 | 3458 / 959 | 3406 / 965 | 3425 / 969 |
| three.js | 514 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 | 524 / 126 | 506 / 123 | 519 / 128 | 617 / 152 |
| lodash-es | 75 / 26 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 | 72 / 25 | 71 / 24 | 85 / 29 | 3 / 1 |
| React + react-dom/server | 227 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 | 226 / 68 | 227 / 67 | 583 / 175 | 585 / 176 |
| rxjs | 24 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 | 143 / 28 | 22 / 7 | 142 / 27 | 175 / 39 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 | 80 / 17 | 3 / 1 | failed | 406 / 67 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 | 541 / 32 | 185 / 21 | 173 / 30 | 271 / 29 |
| Preact app (TSX) | 22 / 9 | 23 / 9 | 23 / 9 | 25 / 10 | 30 / 11 | 25 / 10 | 22 / 9 | 23 / 9 | 28 / 11 |
| Vue app (render functions) | 94 / 36 | 639 / 206 | 630 / 200 | 620 / 197 | 114 / 37 | 631 / 198 | 86 / 33 | 630 / 200 | 635 / 201 |
| Solid app (no JSX) | 48 / 15 | 49 / 15 | 49 / 15 | 44 / 14 | 63 / 16 | 48 / 15 | 3 / 1 | 44 / 14 | 66 / 20 |
| Svelte app (precompiled) | 27 / 10 | 27 / 10 | 24 / 9 | 25 / 9 | 32 / 10 | 25 / 9 | 24 / 9 | 24 / 9 | 98 / 31 |
| Monorepo-style TypeScript app | 59 / 5 | 60 / 5 | 63 / 5 | 340 / 13 | 99 / 6 | 348 / 13 | 59 / 5 | 62 / 5 | 142 / 16 |
| date-fns | 27 / 9 | 28 / 9 | 27 / 8 | 26 / 8 | 40 / 10 | 27 / 8 | 26 / 8 | 26 / 8 | 47 / 13 |
| Zod | 93 / 26 | 99 / 29 | 96 / 26 | 89 / 25 | 120 / 29 | 94 / 26 | 92 / 26 | 141 / 37 | 501 / 102 |
| D3 | 94 / 31 | 98 / 34 | 89 / 30 | 90 / 31 | 114 / 34 | 93 / 32 | 88 / 30 | 122 / 41 | 292 / 94 |

## Does the output run and print the same? (plain)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | same | same | fails | same | fails | same | fails | fails | same |
| three.js | - | - | - | - | - | - | - | - | - |
| lodash-es | - | - | - | - | - | - | - | - | - |
| React + react-dom/server | same | same | same | same | fails | same | same | same | same |
| rxjs | same | same | same | same | same | same | same | same | same |
| 1,500-module app | same | same | same | same | same | same | same | failed | same |
| 500-module TypeScript app | same | same | same | same | same | same | same | same | same |
| Preact app (TSX) | same | same | same | same | same | same | same | same | same |
| Vue app (render functions) | same | same | same | same | same | same | same | same | same |
| Solid app (no JSX) | same | same | same | same | same | same | DIFFERENT | same | same |
| Svelte app (precompiled) | same | same | same | same | same | same | same | same | same |
| Monorepo-style TypeScript app | same | same | same | same | same | same | same | same | same |
| date-fns | same | same | same | same | same | same | same | same | same |
| Zod | same | same | same | same | same | same | same | same | same |
| D3 | same | same | same | same | same | same | same | same | same |

## Warm rebuild in watch mode (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 65.4 | 116 | 596 | 267 | 742 | 801 | 4877 | 64.6 | 110 |
| three.js | 12.5 | 59.2 | 66.2 | 82.0 | 99.7 | 206 | 492 | 24.7 | 17.2 |
| lodash-es | 15.6 | 42.0 | 38.4 | 64.3 | 48.0 | 249 | 239 | 55.9 | 2.4 |
| React + react-dom/server | 6.1 | 17.0 | 34.7 | 31.5 | 40.2 | 87.7 | 195 | 18.2 | 11.1 |
| rxjs | 6.2 | 24.6 | 32.0 | 39.7 | 28.0 | 141 | 110 | 19.9 | 7.0 |
| 1,500-module app | 40.9 | 485 | 144 | 549 | 176 | 1151 | 1321 | failed | 36.8 |
| 500-module TypeScript app | 16.4 | 55.8 | 80.1 | 275 | 94.3 | 458 | 575 | 27.8 | 8.5 |
| Preact app (TSX) | 3.6 | 14.8 | 16.9 | 29.2 | 19.6 | 54.9 | 326 | 12.4 | 2.8 |
| Vue app (render functions) | 8.1 | 38.2 | 78.4 | 56.9 | 48.2 | 197 | 243 | 22.0 | 13.4 |
| Solid app (no JSX) | 4.1 | 19.4 | 21.0 | 33.8 | 28.2 | 75.5 | 45.8 | 14.7 | 4.9 |
| Svelte app (precompiled) | 4.9 | 23.8 | 22.5 | 45.4 | 29.3 | 114 | 98.2 | 20.3 | 5.4 |
| Monorepo-style TypeScript app | 8.5 | 27.4 | 24.1 | 85.9 | 31.4 | 233 | 418 | 23.9 | 5.5 |
| date-fns | 8.9 | 27.0 | 24.1 | 49.8 | 30.1 | 104 | 105 | 17.9 | 5.8 |
| Zod | 9.0 | 26.2 | 28.0 | 43.5 | 37.2 | 139 | 234 | 20.1 | 9.8 |
| D3 | 7.9 | 22.6 | 32.7 | 40.3 | 40.3 | 119 | 234 | 17.6 | 10.5 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 64.1 | 77.1 | 743 | 1740 | 900 | 18592 | 24663 | n/a | 113 |
| three.js | 16.9 | 69.0 | 77.0 | 233 | 110 | 2054 | 1573 | n/a | 17.9 |
| lodash-es | 18.4 | 40.9 | 39.6 | 103 | 48.8 | 903 | 723 | n/a | 2.8 |
| React + react-dom/server | 8.0 | 14.1 | 37.9 | 106 | 45.7 | 1111 | 895 | n/a | 10.5 |
| rxjs | 8.2 | 22.3 | 41.6 | 90.0 | 28.2 | 971 | 315 | n/a | 5.9 |
| 1,500-module app | 44.3 | 485 | 150 | 716 | 179 | 3282 | 1350 | n/a | 36.5 |
| 500-module TypeScript app | 19.7 | 53.6 | 80.3 | 422 | 96.5 | 3343 | 1316 | n/a | 8.2 |
| Preact app (TSX) | 4.8 | 14.6 | 18.2 | 40.5 | 21.2 | 316 | 508 | n/a | 2.7 |
| Vue app (render functions) | 10.2 | 40.3 | 103 | 288 | 59.9 | 2758 | 743 | n/a | 15.8 |
| Solid app (no JSX) | 8.1 | 23.4 | 24.1 | 60.4 | 30.9 | 552 | 125 | n/a | 6.6 |
| Svelte app (precompiled) | 7.7 | 20.4 | 24.9 | 65.7 | 29.4 | 540 | 298 | n/a | 5.2 |
| Monorepo-style TypeScript app | 10.5 | 27.7 | 27.7 | 177 | 32.5 | 2082 | 688 | n/a | 7.8 |
| date-fns | 8.8 | 27.7 | 27.2 | 61.4 | 30.1 | 361 | 304 | n/a | 7.2 |
| Zod | 10.0 | 29.9 | 32.8 | 80.8 | 40.3 | 823 | 687 | n/a | 10.8 |
| D3 | 9.7 | 20.0 | 34.8 | 82.2 | 44.2 | 851 | 692 | n/a | 10.8 |

Each tool runs in its own watch mode on a writable copy of the project, the first build is waited for, then one file of the project is changed 7 times (each change appends one statement), and the time from writing the file to the output file holding the statement is taken; the median of the 7 is shown.

## Development server: start and HMR latency (median ms, lower is better)

Start, until the page and every module it imports have been answered:

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 75.9 | 124 | 223 | 198 | 715 | 881 | 105 |
| lodash-es | 45.1 | 101 | 208 | 210 | 896 | 1228 | 12.0 |
| React + react-dom/server | 33.0 | 85.8 | 175 | 280 | 545 | 709 | 45.7 |
| rxjs | 32.1 | 83.2 | 161 | 270 | 564 | 851 | 24.7 |
| 1,500-module app | 310 | 582 | 479 | 1533 | 1826 | failed | 227 |
| 500-module TypeScript app | 50.0 | 121 | 356 | 729 | 1134 | 1334 | 81.3 |
| Preact app (TSX) | 19.3 | 69.4 | 161 | 271 | 524 | 656 | 21.4 |
| Vue app (render functions) | 32.2 | 90.4 | 180 | 270 | 606 | 700 | 51.7 |
| Solid app (no JSX) | 20.2 | 70.7 | 160 | 261 | 501 | 651 | 23.7 |
| Svelte app (precompiled) | 30.8 | 81.3 | 174 | 272 | 640 | failed | failed |
| Monorepo-style TypeScript app | 31.2 | 89.6 | 208 | 448 | 805 | 955 | 31.3 |
| date-fns | 31.4 | 91.1 | 174 | 212 | 685 | 932 | 42.6 |
| Zod | 43.2 | 84.8 | 176 | 210 | 660 | 792 | 44.8 |
| D3 | 43.5 | 93.6 | 173 | 219 | 673 | 717 | 50.5 |

HMR latency, from writing a change to a module that accepts it to the update notice (the kind of notice is in the second table):

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 19.9 | 57.2 | 29.6 | 1.8 | 39.5 | 6.7 | 31.6 |
| lodash-es | 21.8 | 39.4 | 30.3 | 1.9 | 76.7 | 11.4 | 1.8 |
| React + react-dom/server | 8.4 | 14.7 | 24.1 | 1.9 | 36.1 | 7.1 | 13.4 |
| rxjs | 9.7 | 17.8 | 22.1 | 2.1 | 39.4 | 9.6 | 4.0 |
| 1,500-module app | 138 | 473 | 45.1 | 3.4 | 209 | none | 39.0 |
| 500-module TypeScript app | 19.5 | 54.3 | 27.7 | 16.0 | 70.4 | 10.3 | 9.0 |
| Preact app (TSX) | 2.9 | 11.6 | 22.5 | 2.7 | 37.9 | 7.2 | 4.5 |
| Vue app (render functions) | 9.9 | 13.6 | 23.1 | 2.6 | 38.5 | 7.3 | 12.6 |
| Solid app (no JSX) | 3.2 | 14.1 | 22.0 | 3.0 | 37.8 | 9.7 | 6.2 |
| Svelte app (precompiled) | 9.1 | 20.3 | 24.2 | 1.6 | 41.7 | none | none |
| Monorepo-style TypeScript app | 9.5 | 25.4 | 30.1 | 3.6 | 64.1 | 9.8 | 6.3 |
| date-fns | 10.2 | 28.1 | 23.8 | 2.1 | 50.6 | 9.9 | 6.0 |
| Zod | 13.5 | 114 | 23.1 | 1.8 | 47.8 | 9.8 | 10.6 |
| D3 | 12.8 | 111 | 22.2 | 4.0 | 40.5 | 7.7 | 11.5 |

Notice kinds: Rathkit Pack hmr; esbuild change; rspack ok/warnings; Vite update; webpack ok/warnings; Parcel 2 update; Parcel 3 update. Without a development server: rolldown (no development server (the rolldown command line has none)); Rollup (no development server).

Failed: 1,500-module app on Parcel 2 (Parcel 2 stops with an internal assertion error in its bundler); Svelte app (precompiled) on Parcel 2 (Parcel 2 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container); Svelte app (precompiled) on Parcel 3 (Parcel 3 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container).

Not measured: TypeScript compiler (the TypeScript compiler is a library for Node with no page to serve: it needs fs, path, os and other Node modules, which each tool would have to be set up to stub differently).

## Build plus type check (TypeScript workloads only; wall time and peak memory of every process of the setup)

500-module TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 751 | - | - | 237 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 68.5 | 806 | 738 | 52 | 235 |
| rolldown: rolldown, then `tsc --noEmit` | 160 | 899 | 739 | 185 | 240 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 507 | 1066 | 559 | 157 | 461 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 241 | 996 | 755 | 199 | 237 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 3977 | 4152 | 175 | 494 | 869 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 2357 | 2569 | 212 | 505 | 517 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 1315 | 2040 | 724 | 543 | 541 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 90.7 | 847 | 756 | 77 | 233 |

Preact app (TSX):

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 502 | - | - | 208 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 24.0 | 526 | 502 | 17 | 208 |
| rolldown: rolldown, then `tsc --noEmit` | 103 | 607 | 504 | 131 | 209 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 124 | 790 | 666 | 113 | 391 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 172 | 681 | 509 | 162 | 208 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 673 | 993 | 319 | 212 | 571 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 971 | 1102 | 131 | 310 | 317 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 595 | 1127 | 532 | 336 | 336 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 30.5 | 542 | 511 | 44 | 208 |

Monorepo-style TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 752 | - | - | 232 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 42.1 | 793 | 751 | 32 | 232 |
| rolldown: rolldown, then `tsc --noEmit` | 120 | 880 | 760 | 158 | 232 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 255 | 1066 | 811 | 144 | 447 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 190 | 960 | 770 | 180 | 232 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 2672 | 2896 | 223 | 466 | 856 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 1419 | 1702 | 283 | 381 | 400 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 892 | 1654 | 762 | 490 | 492 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 50.2 | 813 | 762 | 67 | 232 |
<!-- pack-bench:end -->

(*) rspack and Rollup fold the whole generated 1,500-module app to a constant (1 and 3 KB) and Rollup does the same to the Solid app
(3 KB), so their sizes say nothing about those cases. Parcel 3's 3 KB for lodash-es is also not a comparable result: nothing the
entry re-exports was kept. Different tools pick different package entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Time and memory:** Rathkit Pack took the least memory in all 45 cases (15 workloads, three modes) against every tool, and less
time than esbuild, rolldown, rspack, Vite, webpack and Rollup in all 45. Parcel 2 was built in 42 cases and Pack took less time in all
of them. Parcel 3 took less time in three cases, the lodash-es ones, where it kept none of the exports, so that is not the same work; Pack
took less time than Parcel 3 in the other 42.

**Output size:** Against esbuild the output of Rathkit Pack is smaller in 44 of the 45 cases, and smaller in raw bytes and in gzip in every
minified case. esbuild is smaller in two plain (not minified) cases: React by gzip (105 against 108 KB) and Zod by size (199 against 217 KB).

**Where others are smaller:** minified, rolldown, rspack, webpack, Rollup, Parcel 2 and Parcel 3 are each smaller than
Rathkit Pack on the TypeScript compiler (3405 to 3467 KB against 3468 KB, within about 2%); rolldown and Rollup on three.js (513 and 506
against 514 KB); rolldown, webpack and Rollup on lodash-es (73, 72 and 71 against 75 KB); rspack and webpack on React by 1 to 2 KB;
Rollup on rxjs (22 against 24 KB) and on the Vue app (86 against 94 KB); rolldown, rspack, webpack, Rollup and Parcel 2 on the Svelte
app (24 to 25 against 27 KB), date-fns (26 to 27 against 27 KB) and the Solid app (44 to 48 against 49 KB); and rolldown, rspack,
webpack and Rollup on D3 (88 to 93 against 98 KB) and Zod (89 to 96 against 99 KB). Plain, rolldown and Vite are smaller on the
TypeScript compiler (7774 and 7683 against 8216 KB), React (488 and 471 against 578 KB), Zod (182 and 181 against 217 KB) and D3
(153 against 172 KB), Vite is smaller on the Vue app (210 against 252 KB), and Vite and Rollup on rxjs (54 and 56 against 59 KB). Pack
is smallest or tied minified on the 500-module TypeScript app and the monorepo-style app, and in every mode on three.js except the
cases above.

**Parcel:** minified, Parcel 2 is smaller than Rathkit Pack on the TypeScript compiler (3406 against 3468 KB), the Solid app (44
against 49 KB), the Svelte app (24 against 27 KB) and date-fns (26 against 27 KB), and Parcel 3 is smaller on the TypeScript compiler
(3425 KB). Parcel 3 is an alpha.

**Rebuild:** in watch mode Rathkit Pack's warm rebuild is faster than esbuild's in all 30 cases (15 workloads, plain and
minified): 3.6 to 65.4 ms plain and 4.8 to 64.1 ms minified, against 15 to 485 ms plain and 14 to 485 ms minified for esbuild. It is
faster than rolldown, rspack, Vite, webpack and Rollup in all 30. Parcel 2 is faster only on the TypeScript compiler plain; Parcel 3
is faster in 15 of the 30, mostly on the smaller inputs (lodash-es, rxjs, the generated apps, date-fns, the monorepo-style app and the
minified Preact, Solid and Svelte apps). The lowest rebuild time plain was Pack's on 8 of the 15 workloads, Parcel 3's on 6 and Parcel
2's on one; minified, Pack's on 6 and Parcel 3's on 9. On the 1,500-module app Pack takes 41 and 44 ms and esbuild about 485 ms in both.
Rebuilds of webpack and Rollup minified take 19 and 25 s on the TypeScript compiler, against 64 ms for Pack. On Linux Pack hears of a change
through the kernel's file events; on macOS and Windows it still polls, so these figures say nothing about those systems.

**HMR:** latency in Pack's development server is 2.9 to 138 ms (the 1,500-module app). It is lower than esbuild's in all 14 served
workloads (esbuild 12 to 473 ms; esbuild's is a live-reload signal, not HMR), than webpack's in all 14 (36 to 209 ms) and than rspack's
in 13 of 14 (22 to 45 ms; rspack is faster on the 1,500-module app). It is higher than Vite's in all 14 (1.6 to 16.0 ms), than Parcel 2's in
9 of the 12 it has figures for (6.7 to 11.4 ms) and than Parcel 3's in 8 of the 13 (1.8 to 39 ms). Pack's development server started
fastest on 11 of the 14 served workloads; Parcel 3 was fastest on lodash-es, rxjs and the 1,500-module app. Vite does not bundle, so its start
covers the modules of the page only because the measurement asks for all of them. None of this was run in a browser. Parcel 2 has no figure
for the 1,500-module app and the Svelte app, and Parcel 3 none for the Svelte app. The TypeScript compiler has no development-server figure for any tool, for the reason in the method.

**Build plus type check:** Rathkit Pack's own checker (`rkp check`, stage 1) is not measured in this table, so its row has no figure and
nothing is invented for it. The cost of `tsc --noEmit` alone is 0.50 to 0.75 s on these three workloads and 208 to 237 MB. Where a checker runs
in the build, the check adds time on top of the build for every tool; the rows show how much.

## Where another tool wins

Every case in the run above in which another tool took less time, less memory or wrote less than Pack (more than 3% in time and memory, 1% in
size), by kind, with the reason and the issue that follows it. The list is for the maintainer to accept or reject, entry by entry.

| Where | Who is better | By how much | Reason | Issue |
|---|---|---|---|---|
| Build time, lodash-es in all three modes | Parcel 3 | 7.4 to 7.8 ms against 18.6 to 28.2 | Not the same work: it keeps none of what the entry re-exports (3 KB written against 76 KB) | none: not a loss |
| Peak memory | none | Pack is lowest in all 45 cases | | |
| Output size, minified: 33 of the 45 workload and mode cases have a smaller output from one tool or more | Rollup, rolldown, rspack, webpack, Parcel 2 and Vite, by workload (see "Where others are smaller" above) | up to 11% (D3, Zod), mostly 1 to 8% | Not confirmed per case; likely functions inlined across modules and unused parameters removed | [#126](https://github.com/rathkit/pack/issues/126), [#127](https://github.com/rathkit/pack/issues/127) |
| Style sheets, minified alone (not in the tables above) | lightningcss, by up to 2% on 23 of 64 real files | declarations merged across rules; with no browser target, `inset`, `margin-inline` and two-value `overflow` | those shorthands are left out on purpose (support gaps) | [#23](https://github.com/rathkit/pack/issues/23) |
| Warm rebuild in watch mode, 15 of the 30 cases | Parcel 3 | 1.3 to 13 ms against Pack's 3.6 to 44 | not established | [#177](https://github.com/rathkit/pack/issues/177) |
| HMR latency, all 14 workloads | Vite (all 14), Parcel 2 (9 of 12), Parcel 3 (8 of 13), rspack (the 1,500-module app) | Vite 1.6 to 16.0 ms against Pack's 2.9 to 138 | not established | [#178](https://github.com/rathkit/pack/issues/178) |
| Development server start, 3 of the 14 workloads | Parcel 3 | lodash-es 12.0 against 45.1 ms, rxjs 24.7 against 32.1, the 1,500-module app 227 against 310 | not established; Parcel 3 has a persistent cache ([#26](https://github.com/rathkit/pack/issues/26)) | [#179](https://github.com/rathkit/pack/issues/179) |
