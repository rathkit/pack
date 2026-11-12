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
  the change is taken (the output file is polled every 2 ms); the median is shown. Each tool uses its default watcher. Pack polls
  the files its build read every 80 ms and waits 40 ms more for writes to settle, so its figure includes up to 120 ms of that;
  esbuild scans for changes in rounds, so its times vary more (the minimum and the median are in the results file); the others
  use the operating system's file events. Parcel 2 builds in development mode when it watches and has no minified watch mode.
- **Server start and HMR latency:** for the tools that have a development server (Pack `serve --hmr`, esbuild `--serve`, Vite,
  webpack with webpack-dev-server, rspack `serve`, Parcel 2 and 3 `serve`); rolldown and Rollup have none. No browser is used. A
  page load is imitated by asking for the page, the scripts it names and (for Vite, which does not bundle) every module they
  import, six requests at a time; the server has started when all are answered. HMR latency is from writing a change to a module
  that already accepts its own update to the update notice on the tool's own event channel (a WebSocket, or server-sent events);
  what the page then does with it is not measured, and the notices differ in kind (the table says which). The TypeScript compiler is
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
  Rathkit Pack has no type checker and its row has no figure. Each plugin setup was first tried on a deliberate type error to
  confirm that it reports it.
<!-- pack-bench:begin -->
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.58.1-microsoft-standard-WSL2; WSL2, Docker Desktop, laptop on AC. The machine was idle: the load average was 0.56, 1.33, 1.75 before the run and 1.18, 1.43, 1.54 after it. Node 26.13.0; Rathkit Pack 0.2.0-beta.0 at commit `5849969` (release build, sha256 `386afd8a24da`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha), webpack-dev-server 6.1.0, @rspack/dev-server 2.2.2, TypeScript 5.9.3 (tsc), ts-checker-rspack-plugin 1.7.2, fork-ts-checker-webpack-plugin 9.1.0. Run on 2026-11-12 in the pack-bench container, bench commit `7611b23`, image `c3d778c707da`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `7611b23`, build Pack at commit `5849969` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261112T173947Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --runs 7 --rebuild --dev --typecheck --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 211 | 604 | 791 | 407 | 1039 | 1478 | 10608 | 2786 | 667 |
| three.js | 42 | 64 | 150 | 150 | 244 | 601 | 2469 | 714 | 90 |
| lodash-es | 23 | 36 | 116 | 130 | 192 | 759 | 3652 | 1112 | 9 |
| React + react-dom/server | 13 | 20 | 106 | 101 | 181 | 397 | 2241 | 605 | 37 |
| rxjs | 13 | 24 | 108 | 113 | 166 | 567 | 2932 | 774 | 25 |
| 1,500-module app | 101 | 495 | 222 | 600 | 318 | 1877 | 5628 | failed | 200 |
| 500-module TypeScript app | 36 | 60 | 143 | 342 | 230 | 970 | 2301 | 1255 | 76 |
| Preact app (TSX) | 8 | 13 | 97 | 108 | 173 | 398 | 2175 | 620 | 17 |
| Vue app (render functions) | 23 | 52 | 182 | 146 | 217 | 730 | 2539 | 793 | 68 |
| Solid app (no JSX) | 12 | 19 | 103 | 112 | 181 | 428 | 2120 | 623 | 26 |
| Svelte app (precompiled) | 12 | 18 | 110 | 120 | 183 | 547 | 2538 | 701 | 24 |
| Monorepo-style TypeScript app | 15 | 24 | 106 | 162 | 183 | 745 | 2303 | 942 | 27 |
| date-fns | 17 | 32 | 113 | 129 | 192 | 562 | 4808 | 893 | 40 |
| Zod | 19 | 21 | 122 | 124 | 194 | 602 | 2622 | 737 | 33 |
| D3 | 21 | 27 | 131 | 118 | 200 | 578 | 2457 | 690 | 37 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 292 | 433 | 960 | 2085 | 1203 | 21998 | 33884 | 4450 | 1588 |
| three.js | 49 | 74 | 150 | 309 | 245 | 2544 | 3636 | 887 | 138 |
| lodash-es | 26 | 37 | 120 | 168 | 194 | 1504 | 4136 | 1154 | 8 |
| React + react-dom/server | 19 | 21 | 110 | 180 | 186 | 1468 | 2992 | 794 | 65 |
| rxjs | 15 | 24 | 115 | 162 | 171 | 1481 | 3177 | 824 | 30 |
| 1,500-module app | 105 | 498 | 223 | 809 | 317 | 4108 | 5706 | failed | 186 |
| 500-module TypeScript app | 33 | 60 | 147 | 493 | 235 | 3948 | 3086 | 1356 | 87 |
| Preact app (TSX) | 10 | 13 | 99 | 123 | 175 | 739 | 2424 | 656 | 19 |
| Vue app (render functions) | 27 | 54 | 212 | 414 | 218 | 3689 | 3158 | 1022 | 102 |
| Solid app (no JSX) | 17 | 20 | 105 | 140 | 182 | 1051 | 2276 | 654 | 35 |
| Svelte app (precompiled) | 15 | 18 | 112 | 148 | 185 | 1094 | 2803 | 743 | 26 |
| Monorepo-style TypeScript app | 19 | 24 | 113 | 280 | 192 | 2953 | 2623 | 941 | 31 |
| date-fns | 18 | 33 | 114 | 143 | 191 | 888 | 5085 | 909 | 41 |
| Zod | 22 | 24 | 126 | 162 | 198 | 1440 | 3185 | 774 | 42 |
| D3 | 28 | 30 | 131 | 168 | 204 | 1468 | 3026 | 751 | 53 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 451 | 629 | 1019 | 2395 | 1301 | 23116 | 35050 | 5563 | 1790 |
| three.js | 61 | 90 | 169 | 356 | 274 | 2754 | 3810 | 1086 | 154 |
| lodash-es | 34 | 42 | 130 | 184 | 197 | 1565 | 4200 | 1210 | 8 |
| React + react-dom/server | 24 | 26 | 123 | 194 | 191 | 1549 | 3060 | 900 | 69 |
| rxjs | 17 | 31 | 121 | 172 | 174 | 1512 | 3187 | 882 | 32 |
| 1,500-module app | 124 | 509 | 232 | 829 | 328 | 4261 | 5695 | failed | 192 |
| 500-module TypeScript app | 42 | 68 | 161 | 593 | 268 | 4443 | 3492 | 1675 | 89 |
| Preact app (TSX) | 13 | 15 | 101 | 123 | 173 | 793 | 2437 | 659 | 21 |
| Vue app (render functions) | 30 | 102 | 227 | 461 | 232 | 3916 | 3258 | 1246 | 117 |
| Solid app (no JSX) | 18 | 20 | 111 | 146 | 184 | 1106 | 2297 | 689 | 38 |
| Svelte app (precompiled) | 16 | 19 | 111 | 150 | 189 | 1142 | 2801 | 772 | 27 |
| Monorepo-style TypeScript app | 21 | 26 | 112 | 303 | 191 | 3097 | 2663 | 1046 | 33 |
| date-fns | 19 | 33 | 116 | 143 | 189 | 931 | 5055 | 922 | 41 |
| Zod | 27 | 25 | 134 | 174 | 204 | 1499 | 3228 | 832 | 51 |
| D3 | 31 | 30 | 137 | 173 | 214 | 1535 | 3072 | 804 | 59 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 1.8x, rolldown 6.1x, rspack 6.9x, Vite 9.7x, webpack 29.1x, Rollup 130.8x, Parcel 2 41.1x, Parcel 3 2.0x;
minified, esbuild 1.5x, rolldown 5.3x, rspack 9.2x, Vite 8.2x, webpack 73.5x, Rollup 136.1x, Parcel 2 38.0x, Parcel 3 2.1x;
minified with a map, esbuild 1.5x, rolldown 4.7x, rspack 8.3x, Vite 7.1x, webpack 65.0x, Rollup 116.7x, Parcel 2 35.3x, Parcel 3 1.9x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 204 | 265 | 495 | 419 | 458 | 1955 | 3124 | 1382 | 379 |
| three.js | 40 | 60 | 205 | 191 | 234 | 491 | 710 | 568 | 143 |
| lodash-es | 22 | 56 | 237 | 144 | 267 | 351 | 564 | 536 | 27 |
| React + react-dom/server | 18 | 29 | 189 | 131 | 213 | 325 | 596 | 504 | 172 |
| rxjs | 11 | 49 | 224 | 131 | 204 | 340 | 483 | 484 | 90 |
| 1,500-module app | 93 | 380 | 468 | 350 | 496 | 1041 | 844 | failed | 245 |
| 500-module TypeScript app | 28 | 55 | 188 | 177 | 205 | 532 | 596 | 573 | 83 |
| Preact app (TSX) | 8 | 20 | 143 | 113 | 166 | 214 | 435 | 349 | 58 |
| Vue app (render functions) | 18 | 88 | 327 | 178 | 214 | 541 | 616 | 581 | 203 |
| Solid app (no JSX) | 10 | 22 | 163 | 119 | 190 | 242 | 413 | 380 | 81 |
| Svelte app (precompiled) | 10 | 27 | 200 | 122 | 226 | 234 | 466 | 425 | 90 |
| Monorepo-style TypeScript app | 13 | 34 | 162 | 155 | 190 | 465 | 507 | 520 | 73 |
| date-fns | 13 | 41 | 196 | 123 | 220 | 212 | 770 | 508 | 79 |
| Zod | 18 | 42 | 264 | 132 | 283 | 316 | 547 | 456 | 141 |
| D3 | 19 | 41 | 272 | 132 | 299 | 299 | 553 | 452 | 155 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 3477 / 975 | 3515 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 | 3467 / 959 | 3458 / 959 | 3406 / 965 | 3425 / 969 |
| three.js | 515 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 | 524 / 126 | 506 / 123 | 519 / 128 | 617 / 152 |
| lodash-es | 76 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 | 72 / 25 | 71 / 24 | 85 / 29 | 3 / 1 |
| React + react-dom/server | 228 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 | 226 / 68 | 227 / 67 | 583 / 175 | 585 / 176 |
| rxjs | 25 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 | 143 / 28 | 22 / 7 | 142 / 27 | 175 / 39 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 | 80 / 17 | 3 / 1 | failed | 406 / 67 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 | 541 / 32 | 185 / 21 | 173 / 30 | 271 / 29 |
| Preact app (TSX) | 24 / 10 | 23 / 9 | 23 / 9 | 25 / 10 | 30 / 11 | 25 / 10 | 22 / 9 | 23 / 9 | 28 / 11 |
| Vue app (render functions) | 95 / 36 | 639 / 206 | 630 / 200 | 620 / 197 | 114 / 37 | 631 / 198 | 86 / 33 | 630 / 200 | 635 / 201 |
| Solid app (no JSX) | 50 / 15 | 49 / 15 | 49 / 15 | 44 / 14 | 63 / 16 | 48 / 15 | 3 / 1 | 44 / 14 | 66 / 20 |
| Svelte app (precompiled) | 29 / 10 | 27 / 10 | 24 / 9 | 25 / 9 | 32 / 10 | 25 / 9 | 24 / 9 | 24 / 9 | 98 / 31 |
| Monorepo-style TypeScript app | 59 / 5 | 60 / 5 | 63 / 5 | 340 / 13 | 99 / 6 | 348 / 13 | 59 / 5 | 62 / 5 | 142 / 16 |
| date-fns | 28 / 9 | 28 / 9 | 27 / 8 | 26 / 8 | 40 / 10 | 27 / 8 | 26 / 8 | 26 / 8 | 47 / 13 |
| Zod | 100 / 28 | 99 / 29 | 96 / 26 | 89 / 25 | 120 / 29 | 94 / 26 | 92 / 26 | 141 / 37 | 501 / 102 |
| D3 | 98 / 33 | 98 / 34 | 89 / 30 | 90 / 31 | 114 / 34 | 93 / 32 | 88 / 30 | 122 / 41 | 292 / 94 |

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
| TypeScript compiler | 189 | 192 | 683 | 320 | 832 | 919 | 5845 | 82.5 | 129 |
| three.js | 55.6 | 61.0 | 66.0 | 78.5 | 99.8 | 207 | 500 | 24.2 | 23.2 |
| lodash-es | 60.1 | 42.9 | 35.5 | 66.6 | 46.4 | 251 | 241 | 50.9 | 2.8 |
| React + react-dom/server | 47.4 | 16.8 | 33.8 | 32.5 | 41.1 | 94.2 | 204 | 17.1 | 10.8 |
| rxjs | 51.4 | 23.6 | 30.5 | 42.5 | 28.4 | 139 | 111 | 20.4 | 5.8 |
| 1,500-module app | 126 | 489 | 148 | 554 | 181 | 1173 | 1309 | failed | 32.6 |
| 500-module TypeScript app | 72.8 | 59.4 | 83.3 | 297 | 105 | 528 | 620 | 36.1 | 9.8 |
| Preact app (TSX) | 124 | 17.0 | 17.0 | 29.7 | 20.5 | 59.6 | 331 | 14.4 | 4.8 |
| Vue app (render functions) | 126 | 42.2 | 91.2 | 60.8 | 54.1 | 223 | 276 | 25.5 | 16.0 |
| Solid app (no JSX) | 124 | 21.2 | 24.6 | 35.9 | 30.7 | 82.3 | 50.2 | 16.5 | 7.1 |
| Svelte app (precompiled) | 50.5 | 21.9 | 22.4 | 44.7 | 30.4 | 123 | 110 | 19.4 | 6.6 |
| Monorepo-style TypeScript app | 54.5 | 29.3 | 28.0 | 92.2 | 34.1 | 247 | 441 | 31.1 | 8.3 |
| date-fns | 52.5 | 27.3 | 24.5 | 53.5 | 33.0 | 126 | 121 | 22.0 | 9.6 |
| Zod | 125 | 30.0 | 30.2 | 47.0 | 39.2 | 162 | 270 | 21.0 | 11.1 |
| D3 | 130 | 22.0 | 33.1 | 41.0 | 43.0 | 141 | 273 | 20.8 | 9.9 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 265 | 91.4 | 851 | 2090 | 1002 | 22136 | 24887 | n/a | 112 |
| three.js | 62.5 | 66.7 | 74.0 | 241 | 111 | 2069 | 1582 | n/a | 18.4 |
| lodash-es | 62.1 | 43.0 | 39.3 | 109 | 52.5 | 940 | 711 | n/a | 2.5 |
| React + react-dom/server | 50.7 | 14.6 | 42.8 | 105 | 49.7 | 1088 | 890 | n/a | 10.8 |
| rxjs | 53.8 | 23.4 | 41.1 | 92.1 | 28.2 | 960 | 315 | n/a | 7.4 |
| 1,500-module app | 130 | 488 | 148 | 731 | 181 | 3374 | 1370 | n/a | 34.3 |
| 500-module TypeScript app | 74.5 | 59.9 | 86.0 | 485 | 107 | 3733 | 1439 | n/a | 9.2 |
| Preact app (TSX) | 48.1 | 15.6 | 17.6 | 44.0 | 22.9 | 347 | 549 | n/a | 4.7 |
| Vue app (render functions) | 130 | 42.6 | 123 | 338 | 65.4 | 3165 | 867 | n/a | 16.9 |
| Solid app (no JSX) | 125 | 24.7 | 28.4 | 61.8 | 31.5 | 638 | 145 | n/a | 6.8 |
| Svelte app (precompiled) | 123 | 21.9 | 23.3 | 67.1 | 30.0 | 628 | 345 | n/a | 6.2 |
| Monorepo-style TypeScript app | 59.5 | 31.3 | 24.9 | 202 | 35.4 | 2352 | 756 | n/a | 6.6 |
| date-fns | 131 | 31.8 | 27.8 | 64.5 | 32.4 | 412 | 338 | n/a | 6.1 |
| Zod | 131 | 31.4 | 33.9 | 83.1 | 41.8 | 955 | 788 | n/a | 11.9 |
| D3 | 132 | 21.7 | 39.0 | 89.1 | 49.4 | 973 | 784 | n/a | 10.5 |

Each tool runs in its own watch mode on a writable copy of the project, the first build is waited for, then one file of the project is changed 7 times (each change appends one statement), and the time from writing the file to the output file holding the statement is taken; the median of the 7 is shown.

## Development server: start and HMR latency (median ms, lower is better)

Start, until the page and every module it imports have been answered:

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 77.7 | 131 | 226 | 202 | 716 | 905 | 107 |
| lodash-es | 50.8 | 104 | 209 | 208 | 888 | 1258 | 12.3 |
| React + react-dom/server | 34.1 | 86.3 | 178 | 278 | 551 | 730 | 44.2 |
| rxjs | 40.6 | 84.1 | 159 | 271 | 565 | 863 | 24.7 |
| 1,500-module app | 309 | 587 | 500 | 1553 | 1850 | failed | 230 |
| 500-module TypeScript app | 61.0 | 129 | 401 | 819 | 1294 | 1546 | 92.4 |
| Preact app (TSX) | 21.2 | 72.3 | 187 | 307 | 644 | 767 | 30.4 |
| Vue app (render functions) | 42.5 | 92.7 | 200 | 296 | 686 | 784 | 56.3 |
| Solid app (no JSX) | 21.4 | 71.1 | 184 | 285 | 567 | 736 | 30.9 |
| Svelte app (precompiled) | 33.0 | 82.1 | 192 | 304 | 723 | failed | failed |
| Monorepo-style TypeScript app | 32.2 | 92.9 | 237 | 494 | 918 | 1050 | 35.4 |
| date-fns | 35.3 | 94.8 | 198 | 241 | 763 | 1065 | 49.0 |
| Zod | 45.1 | 86.7 | 200 | 236 | 758 | 894 | 50.1 |
| D3 | 44.6 | 95.3 | 191 | 242 | 762 | 812 | 55.9 |

HMR latency, from writing a change to a module that accepts it to the update notice (the kind of notice is in the second table):

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 115 | 59.8 | 30.8 | 1.8 | 39.6 | 6.9 | 34.5 |
| lodash-es | 116 | 40.3 | 29.8 | 2.1 | 76.1 | 11.5 | 2.1 |
| React + react-dom/server | 96.1 | 13.3 | 23.9 | 2.5 | 36.2 | 7.7 | 13.3 |
| rxjs | 108 | 18.9 | 23.5 | 1.9 | 39.2 | 8.2 | 4.4 |
| 1,500-module app | 324 | 488 | 53.4 | 3.4 | 202 | none | 41.6 |
| 500-module TypeScript app | 124 | 57.4 | 30.8 | 16.6 | 79.8 | 11.6 | 10.4 |
| Preact app (TSX) | 87.9 | 13.0 | 23.9 | 2.0 | 41.7 | 10.2 | 5.5 |
| Vue app (render functions) | 96.6 | 15.0 | 23.7 | 2.4 | 44.2 | 9.2 | 16.3 |
| Solid app (no JSX) | 86.6 | 14.1 | 23.9 | 1.5 | 41.4 | 9.4 | 5.7 |
| Svelte app (precompiled) | 94.0 | 17.9 | 25.7 | 2.1 | 46.2 | none | none |
| Monorepo-style TypeScript app | 104 | 25.9 | 30.8 | 4.6 | 73.7 | 11.6 | 9.7 |
| date-fns | 103 | 29.1 | 26.3 | 2.5 | 59.2 | 9.8 | 9.3 |
| Zod | 102 | 28.8 | 23.6 | 5.0 | 51.5 | 9.9 | 12.2 |
| D3 | 107 | 20.0 | 28.5 | 2.4 | 42.3 | 8.7 | 12.2 |

Notice kinds: Rathkit Pack hmr; esbuild change; rspack ok/warnings; Vite update; webpack ok/warnings; Parcel 2 update; Parcel 3 update. Without a development server, so not in these tables: rolldown, Rollup.

Failed: 1,500-module app on Parcel 2 (Parcel 2 stops with an internal assertion error in its bundler); Svelte app (precompiled) on Parcel 2 (Parcel 2 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container); Svelte app (precompiled) on Parcel 3 (Parcel 3 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container).

Not measured: TypeScript compiler (the TypeScript compiler is a library for Node with no page to serve: it needs fs, path, os and other Node modules, which each tool would have to be set up to stub differently).

## Build plus type check (TypeScript workloads only; wall time and peak memory of every process of the setup)

500-module TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 835 | - | - | 238 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 75.1 | 909 | 833 | 52 | 235 |
| rolldown: rolldown, then `tsc --noEmit` | 178 | 1020 | 843 | 183 | 238 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 571 | 1178 | 607 | 157 | 462 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 272 | 1126 | 854 | 197 | 239 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 4478 | 4682 | 204 | 493 | 869 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 2647 | 2868 | 221 | 506 | 517 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 1510 | 2354 | 843 | 543 | 543 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 102 | 938 | 835 | 76 | 235 |

Preact app (TSX):

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 614 | - | - | 208 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 32.8 | 641 | 608 | 17 | 208 |
| rolldown: rolldown, then `tsc --noEmit` | 117 | 675 | 559 | 133 | 208 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 137 | 979 | 842 | 113 | 391 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 195 | 738 | 543 | 157 | 208 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 718 | 1091 | 373 | 214 | 572 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 1169 | 1336 | 166 | 309 | 319 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 713 | 1282 | 569 | 337 | 337 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 35.3 | 638 | 603 | 44 | 209 |

Monorepo-style TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 855 | - | - | 231 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 51.6 | 891 | 840 | 32 | 232 |
| rolldown: rolldown, then `tsc --noEmit` | 132 | 999 | 867 | 159 | 231 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 296 | 1185 | 888 | 144 | 446 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 215 | 1072 | 857 | 182 | 233 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 3063 | 3239 | 176 | 468 | 856 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 1665 | 1936 | 271 | 378 | 402 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 1013 | 1866 | 853 | 494 | 494 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 55.0 | 901 | 846 | 65 | 231 |
<!-- pack-bench:end -->

(*) rspack and Rollup fold the whole generated 1,500-module app to a constant (1 and 3 KB) and Rollup does the same to the Solid app
(3 KB), so their sizes say nothing about those cases. Parcel 3's 3 KB for lodash-es is also not a comparable result: nothing the
entry re-exports was kept. Different tools pick different package entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Time and memory:** Rathkit Pack took the least memory in all 45 cases (15 workloads, three modes) against every tool, and less
time than rolldown, rspack, Vite, webpack and Rollup in all 45. Against esbuild it took less time in 43 of the 45 cases; esbuild was
faster in two, by 1.8 ms or less (Zod minified with a map 26.7 against 25.0 ms, D3 minified with a map 31.4 against 29.6 ms). Parcel 3 took less time in 42 cases and was faster in the three lodash-es cases, where it kept none of the
exports, so those are not the same work; Parcel 2 was built in 42 cases, and Pack took less time in all of them.

**Output size:** Against esbuild the output of Rathkit Pack is smaller in 34 of the 45 cases. esbuild is smaller minified on the
Preact app (23 against 24 KB), the Solid app (49 against 50 KB), the Svelte app (27 against 29 KB), date-fns (28 KB, gzip 9 KB, about
equal) and Zod (99 against 100 KB, but gzip is larger), and plain on Zod (199 against 217 KB).

**Where others are smaller:** minified, rolldown, rspack, webpack, Rollup, Parcel 2 and Parcel 3 are each smaller than
Rathkit Pack on the TypeScript compiler (3405 to 3472 KB against 3477 KB, within about 2%; gzip 959 to 970 against 975); rolldown
and Rollup on three.js (513 and 506 against 515 KB); rolldown, webpack and Rollup on lodash-es (73, 72 and 71 against 76 KB);
rolldown, rspack, webpack and Rollup on React by up to 3 KB; Rollup on rxjs (22 against 25 KB) and on the Vue app (86 against 95 KB);
rolldown, rspack, webpack, Rollup and Parcel 2 on the Svelte app (24 to 25 against 29 KB), date-fns (26 to 27 against 28 KB) and the
Solid app (44 to 49 against 50 KB); and rolldown, rspack, webpack and Rollup on D3 (88 to 93 against 98 KB) and Zod (89 to 96 against
100 KB). Plain (not minified), rolldown and Vite are smaller on the TypeScript compiler (7774 and 7683 against 8216 KB), React (488 and 471
against 578 KB) and D3 (153 against 172 KB), Vite is smaller on the Vue app (210 against 252 KB), and Vite and Rollup on rxjs (54 and 56
against 60 KB). Pack's output is smallest or tied on three.js, the 500-module TypeScript app and the monorepo-style app in every mode
except the cases above.

**Parcel:** minified, Parcel 2 is smaller than Rathkit Pack on the TypeScript compiler (3406 against 3477 KB; gzip 965 against 975), the
Preact, Solid, Svelte and date-fns apps (by 1 to 6 KB), and Parcel 3 is smaller on the TypeScript compiler (3425 KB). Parcel 3 is larger on
React, rxjs, Vue, Zod, D3, Svelte, date-fns, the monorepo-style app and both generated apps (its plain build of the 1,500-module app is
6990 KB, against 126 KB for Rathkit Pack). Parcel 3 is an alpha. Against Parcel 2 it was between 2.8x and 37.4x faster on the inputs both
built, apart from the lodash-es cases where it kept nothing (124x to 152x), and used less memory in all 42; this was not close to the 100x
its announcement gives as the best case, which was measured on other, larger inputs.

**Rebuild and HMR:** in watch mode Rathkit Pack's warm rebuild is not the fastest, and the roadmap goal here is a measurement, not a win.
Plain, Parcel 3 had the lowest rebuild time on 14 of the 15 workloads and Parcel 2 on the other (the TypeScript compiler); minified,
Parcel 3 on 14 and esbuild on one. Pack's rebuild is 47 to 189 ms plain and 48 to 265 ms minified, and it was slower than esbuild's
on 12 of the 15 workloads plain and 13 minified (esbuild 17 to 489 ms plain; the 1,500-module app is where esbuild is slowest). Part of Pack's time
is how it notices a change: it polls the files its build read every 80 ms and waits 40 ms more, so up to 120 ms of each figure is
that; the other tools use file events, except esbuild, which scans in rounds. Rebuilds of webpack and Rollup minified take 22 and
25 s on the TypeScript compiler, against 265 ms for Pack. HMR latency in Pack's development server is 87 to 324 ms (the 1,500-module
app), slower than every other tool's on 13 of the 14 served workloads (it is ahead of esbuild's live reload only on the 1,500-module
app, where esbuild takes 488 ms; that is not HMR, it signals a change); rspack is 24 to 53 ms, webpack 36 to 202 ms, Vite 1.5 to 17 ms, Parcel 2
7 to 12 ms and Parcel 3 2.1 to 42 ms. Pack's development server started fastest on 11 of the 14 served workloads; Parcel 3 was fastest on
lodash-es, rxjs and the 1,500-module app. Vite does not bundle, so its start covers the modules of the page only because the measurement
asks for all of them. None of this was run in a browser. Three development-server cells and one rebuild cell have no figure: Parcel 2 on
the 1,500-module app (an internal assertion error in its bundler; the plain watch mode of that app never produced a first build either) and on the
Svelte app, and Parcel 3 on the Svelte app (both need the `buffer` polyfill package for a Node global that Svelte's server runtime
uses, and package installs are off in the container). The TypeScript compiler has no development-server figure for any tool, for the
reason in the method.

**Build plus type check:** Rathkit Pack has no type checker, so it has no figure in that table and nothing is invented for it; the cost
of `tsc --noEmit` alone is 0.5 to 0.75 s on these three workloads and 230 MB. Where a checker runs in the build, the check adds 0.14 to 0.3 s
for webpack and Rollup (the build is slow enough to hide most of it), 0.5 to 0.8 s for rspack and 0.5 to 0.77 s as a separate step. The
issue for this table (3.13) stays open until Pack has a checker of its own to put in the column.
