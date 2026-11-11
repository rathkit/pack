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
  not served, because it needs Node's own modules.
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
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.58.1-microsoft-standard-WSL2; WSL2, Docker Desktop, laptop on AC. The machine was idle: the load average was 0.74, 1.13, 1.36 before the run and 1.44, 1.45, 1.47 after it. Node 26.13.0; Rathkit Pack 0.1.0-alpha.2 at commit `429159f` (release build, sha256 `88ab2d7c1102`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha), webpack-dev-server 6.1.0, @rspack/dev-server 2.2.2, TypeScript 5.9.3 (tsc), ts-checker-rspack-plugin 1.7.2, fork-ts-checker-webpack-plugin 9.1.0. Run on 2026-11-11 in the pack-bench container, bench commit `363891f`, image `61d5cb3e6693`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `363891f`, build Pack at commit `429159f` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261111T204457Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --runs 7 --rebuild --dev --typecheck --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 177 | 351 | 669 | 342 | 878 | 1267 | 8636 | 2263 | 568 |
| three.js | 42 | 64 | 148 | 150 | 232 | 600 | 2445 | 698 | 87 |
| lodash-es | 21 | 34 | 115 | 126 | 186 | 749 | 3600 | 1076 | 9 |
| React + react-dom/server | 12 | 19 | 105 | 99 | 175 | 397 | 2183 | 593 | 35 |
| rxjs | 11 | 22 | 107 | 105 | 164 | 562 | 2910 | 754 | 23 |
| 1,500-module app | 97 | 480 | 214 | 577 | 311 | 1857 | 5571 | failed | 190 |
| 500-module TypeScript app | 28 | 58 | 138 | 330 | 224 | 940 | 2233 | 1207 | 77 |
| Preact app (TSX) | 6 | 11 | 86 | 97 | 156 | 346 | 1893 | 573 | 14 |
| Vue app (render functions) | 17 | 43 | 149 | 126 | 184 | 626 | 2215 | 687 | 55 |
| Solid app (no JSX) | 9 | 16 | 93 | 101 | 160 | 369 | 1862 | 548 | 24 |
| Svelte app (precompiled) | 10 | 16 | 101 | 112 | 166 | 473 | 2217 | 622 | 21 |
| Monorepo-style TypeScript app | 13 | 22 | 98 | 143 | 166 | 654 | 2012 | 826 | 24 |
| date-fns | 13 | 28 | 102 | 118 | 166 | 489 | 4285 | 782 | 34 |
| Zod | 17 | 20 | 108 | 110 | 176 | 534 | 2315 | 628 | 27 |
| D3 | 19 | 25 | 112 | 106 | 179 | 507 | 2177 | 602 | 31 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 245 | 367 | 821 | 1730 | 1025 | 18661 | 27371 | 3892 | 1469 |
| three.js | 48 | 73 | 147 | 307 | 245 | 2515 | 3597 | 877 | 135 |
| lodash-es | 24 | 36 | 115 | 166 | 191 | 1467 | 4097 | 1133 | 7 |
| React + react-dom/server | 17 | 20 | 108 | 176 | 179 | 1444 | 2941 | 780 | 62 |
| rxjs | 13 | 23 | 113 | 157 | 167 | 1449 | 3150 | 812 | 28 |
| 1,500-module app | 102 | 488 | 217 | 783 | 349 | 4070 | 5618 | failed | 178 |
| 500-module TypeScript app | 32 | 56 | 145 | 473 | 231 | 3871 | 2991 | 1253 | 75 |
| Preact app (TSX) | 8 | 10 | 89 | 110 | 158 | 641 | 2098 | 595 | 17 |
| Vue app (render functions) | 22 | 48 | 170 | 354 | 197 | 3220 | 2766 | 901 | 89 |
| Solid app (no JSX) | 12 | 17 | 94 | 129 | 162 | 898 | 1983 | 580 | 30 |
| Svelte app (precompiled) | 11 | 16 | 98 | 133 | 168 | 949 | 2463 | 650 | 24 |
| Monorepo-style TypeScript app | 15 | 22 | 99 | 238 | 168 | 2551 | 2323 | 855 | 28 |
| date-fns | 15 | 29 | 101 | 132 | 169 | 773 | 4564 | 805 | 36 |
| Zod | 21 | 20 | 113 | 147 | 183 | 1258 | 2798 | 690 | 36 |
| D3 | 24 | 26 | 115 | 150 | 185 | 1298 | 2666 | 665 | 48 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 326 | 546 | 998 | 2007 | 1123 | 19129 | 29138 | 4779 | 1519 |
| three.js | 58 | 87 | 166 | 348 | 260 | 2695 | 3693 | 1057 | 145 |
| lodash-es | 32 | 41 | 122 | 180 | 195 | 1539 | 4148 | 1170 | 7 |
| React + react-dom/server | 21 | 24 | 115 | 191 | 188 | 1532 | 2997 | 879 | 71 |
| rxjs | 16 | 29 | 121 | 167 | 170 | 1495 | 3151 | 832 | 30 |
| 1,500-module app | 117 | 502 | 226 | 808 | 321 | 4140 | 5634 | failed | 183 |
| 500-module TypeScript app | 39 | 58 | 152 | 531 | 233 | 3862 | 3066 | 1425 | 76 |
| Preact app (TSX) | 9 | 13 | 92 | 112 | 158 | 668 | 2114 | 604 | 19 |
| Vue app (render functions) | 26 | 88 | 188 | 394 | 199 | 3403 | 2827 | 1070 | 101 |
| Solid app (no JSX) | 14 | 18 | 97 | 129 | 168 | 950 | 1984 | 608 | 33 |
| Svelte app (precompiled) | 13 | 18 | 103 | 141 | 172 | 980 | 2479 | 672 | 29 |
| Monorepo-style TypeScript app | 18 | 23 | 104 | 262 | 173 | 2674 | 2340 | 885 | 30 |
| date-fns | 17 | 30 | 106 | 135 | 172 | 808 | 4541 | 813 | 35 |
| Zod | 24 | 24 | 114 | 154 | 182 | 1296 | 2842 | 733 | 40 |
| D3 | 29 | 27 | 120 | 157 | 187 | 1351 | 2689 | 730 | 49 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 1.9x, rolldown 6.6x, rspack 7.5x, Vite 10.5x, webpack 31.4x, Rollup 141.3x, Parcel 2 44.4x, Parcel 3 2.1x;
minified, esbuild 1.6x, rolldown 5.6x, rspack 9.8x, Vite 8.9x, webpack 78.0x, Rollup 145.0x, Parcel 2 40.9x, Parcel 3 2.2x;
minified with a map, esbuild 1.5x, rolldown 5.0x, rspack 8.8x, Vite 7.5x, webpack 67.5x, Rollup 122.3x, Parcel 2 36.9x, Parcel 3 1.9x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 202 | 265 | 521 | 413 | 470 | 1945 | 3103 | 1385 | 366 |
| three.js | 38 | 60 | 205 | 193 | 234 | 491 | 711 | 566 | 140 |
| lodash-es | 22 | 57 | 235 | 145 | 265 | 349 | 567 | 539 | 27 |
| React + react-dom/server | 18 | 29 | 199 | 131 | 216 | 324 | 600 | 505 | 177 |
| rxjs | 11 | 49 | 229 | 131 | 207 | 340 | 487 | 487 | 87 |
| 1,500-module app | 93 | 380 | 470 | 354 | 503 | 1055 | 845 | failed | 244 |
| 500-module TypeScript app | 28 | 55 | 193 | 179 | 205 | 533 | 597 | 573 | 81 |
| Preact app (TSX) | 8 | 20 | 147 | 113 | 162 | 214 | 435 | 348 | 59 |
| Vue app (render functions) | 18 | 88 | 315 | 178 | 212 | 540 | 609 | 595 | 200 |
| Solid app (no JSX) | 10 | 22 | 155 | 119 | 191 | 241 | 415 | 377 | 79 |
| Svelte app (precompiled) | 10 | 27 | 203 | 122 | 222 | 235 | 464 | 422 | 89 |
| Monorepo-style TypeScript app | 13 | 34 | 170 | 155 | 190 | 467 | 506 | 521 | 77 |
| date-fns | 13 | 41 | 200 | 123 | 219 | 213 | 768 | 521 | 75 |
| Zod | 18 | 42 | 248 | 132 | 280 | 317 | 549 | 455 | 140 |
| D3 | 19 | 41 | 266 | 132 | 289 | 302 | 553 | 448 | 159 |

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
| TypeScript compiler | 184 | 121 | 589 | 266 | 730 | 772 | 4838 | 69.7 | 125 |
| three.js | 58.6 | 60.2 | 68.3 | 73.9 | 101 | 197 | 468 | 23.0 | 17.4 |
| lodash-es | 59.3 | 42.5 | 37.9 | 63.9 | 48.0 | 234 | 234 | 49.5 | 2.9 |
| React + react-dom/server | 49.8 | 15.8 | 34.4 | 31.5 | 42.6 | 89.0 | 197 | 17.4 | 11.2 |
| rxjs | 53.1 | 22.4 | 33.1 | 40.5 | 27.6 | 136 | 113 | 20.1 | 6.5 |
| 1,500-module app | 125 | 477 | 143 | 547 | 177 | 1134 | 1292 | failed | 34.0 |
| 500-module TypeScript app | 69.1 | 55.3 | 78.2 | 273 | 95.8 | 462 | 577 | 29.3 | 8.7 |
| Preact app (TSX) | 45.4 | 15.1 | 19.0 | 29.3 | 20.8 | 55.3 | 319 | 12.8 | 4.8 |
| Vue app (render functions) | 50.6 | 38.3 | 85.1 | 55.5 | 48.8 | 191 | 238 | 21.9 | 14.1 |
| Solid app (no JSX) | 46.7 | 20.3 | 23.3 | 32.4 | 29.5 | 68.7 | 44.7 | 12.5 | 7.4 |
| Svelte app (precompiled) | 127 | 19.8 | 23.6 | 43.4 | 27.2 | 110 | 96.3 | 16.1 | 5.5 |
| Monorepo-style TypeScript app | 55.2 | 26.1 | 24.1 | 82.8 | 35.2 | 223 | 410 | 23.0 | 7.4 |
| date-fns | 53.4 | 27.0 | 27.0 | 49.1 | 33.1 | 103 | 107 | 20.6 | 5.8 |
| Zod | 50.6 | 24.2 | 30.3 | 45.0 | 38.8 | 136 | 228 | 19.1 | 10.5 |
| D3 | 131 | 23.9 | 37.0 | 39.8 | 40.7 | 112 | 225 | 16.2 | 8.7 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 240 | 77.7 | 747 | 1618 | 891 | 18399 | 24270 | n/a | 108 |
| three.js | 61.4 | 65.8 | 77.7 | 237 | 111 | 2004 | 1552 | n/a | 18.2 |
| lodash-es | 61.8 | 42.0 | 39.0 | 101 | 46.3 | 904 | 690 | n/a | 2.5 |
| React + react-dom/server | 51.7 | 15.6 | 38.0 | 105 | 47.4 | 1077 | 878 | n/a | 11.6 |
| rxjs | 53.6 | 22.2 | 42.1 | 91.2 | 28.7 | 949 | 317 | n/a | 6.2 |
| 1,500-module app | 128 | 496 | 146 | 707 | 181 | 3275 | 1307 | n/a | 36.1 |
| 500-module TypeScript app | 71.8 | 53.7 | 81.9 | 416 | 105 | 3253 | 1334 | n/a | 8.7 |
| Preact app (TSX) | 49.4 | 16.3 | 19.6 | 41.2 | 23.3 | 306 | 493 | n/a | 5.1 |
| Vue app (render functions) | 52.3 | 39.7 | 102 | 285 | 57.8 | 2687 | 738 | n/a | 14.8 |
| Solid app (no JSX) | 127 | 23.8 | 24.8 | 57.9 | 29.8 | 546 | 126 | n/a | 6.0 |
| Svelte app (precompiled) | 50.5 | 20.7 | 26.4 | 64.0 | 29.8 | 529 | 287 | n/a | 5.5 |
| Monorepo-style TypeScript app | 60.5 | 25.6 | 29.1 | 176 | 33.7 | 2050 | 664 | n/a | 5.5 |
| date-fns | 53.0 | 26.8 | 28.5 | 60.7 | 31.8 | 360 | 294 | n/a | 4.9 |
| Zod | 55.0 | 29.0 | 32.4 | 79.3 | 43.0 | 807 | 670 | n/a | 11.0 |
| D3 | 60.6 | 20.6 | 37.8 | 81.5 | 45.0 | 843 | 665 | n/a | 9.6 |

Each tool runs in its own watch mode on a writable copy of the project, the first build is waited for, then one file of the project is changed 7 times (each change appends one statement), and the time from writing the file to the output file holding the statement is taken; the median of the 7 is shown.

## Development server: start and HMR latency (median ms, lower is better)

Start, until the page and every module it imports have been answered:

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| three.js | 74.9 | 123 | 215 | 193 | 698 | 863 | 103 |
| lodash-es | 45.4 | 99.0 | 193 | 206 | 879 | 1212 | 11.3 |
| React + react-dom/server | 31.6 | 83.7 | 173 | 273 | 534 | 704 | 42.1 |
| rxjs | 38.3 | 82.3 | 154 | 264 | 550 | 820 | 24.2 |
| 1,500-module app | 298 | 568 | 400 | 1514 | 1792 | failed | 221 |
| 500-module TypeScript app | 49.2 | 121 | 319 | 725 | 1112 | 1332 | 80.9 |
| Preact app (TSX) | 17.2 | 68.4 | 155 | 263 | 512 | failed | 19.9 |
| Vue app (render functions) | 31.0 | 89.8 | 174 | 262 | 589 | 672 | 50.9 |
| Solid app (no JSX) | 19.2 | 69.8 | 155 | 254 | 488 | failed | 20.8 |
| Svelte app (precompiled) | 29.2 | 79.8 | 170 | 269 | 629 | failed | failed |
| Monorepo-style TypeScript app | 29.1 | 84.5 | 197 | 428 | 795 | failed | 32.2 |
| date-fns | 32.6 | 91.3 | 171 | 204 | 663 | 923 | 40.9 |
| Zod | 41.3 | 83.3 | 167 | 211 | 653 | 771 | 42.7 |
| D3 | 41.6 | 92.2 | 174 | 209 | 658 | 707 | 44.0 |

HMR latency, from writing a change to a module that accepts it to the update notice (the kind of notice is in the second table):

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| three.js | 114 | 57.4 | 29.4 | 1.8 | 39.4 | 7.0 | 35.0 |
| lodash-es | 116 | 40.7 | 30.0 | 2.1 | 72.7 | 10.7 | 2.4 |
| React + react-dom/server | 95.4 | 13.9 | 23.3 | 2.1 | 36.4 | 7.0 | 13.8 |
| rxjs | 109 | 17.6 | 21.4 | 2.0 | 38.2 | 8.5 | 4.6 |
| 1,500-module app | 318 | 469 | 47.4 | 3.5 | 202 | failed | 36.9 |
| 500-module TypeScript app | 119 | 51.9 | 26.3 | 15.5 | 69.8 | 9.1 | 9.4 |
| Preact app (TSX) | 88.7 | 12.1 | 21.7 | 1.6 | 38.1 | failed | 4.1 |
| Vue app (render functions) | 95.9 | 14.2 | 22.5 | 1.4 | 40.3 | 7.6 | 13.5 |
| Solid app (no JSX) | 85.2 | 14.1 | 20.8 | 1.4 | 39.1 | failed | 7.1 |
| Svelte app (precompiled) | 93.9 | 19.6 | failed | 2.8 | failed | failed | failed |
| Monorepo-style TypeScript app | 104 | 22.3 | 28.3 | 3.7 | 62.0 | failed | 7.1 |
| date-fns | 102 | 25.4 | 24.5 | 1.9 | 49.1 | 8.6 | 5.2 |
| Zod | 101 | 22.3 | 22.8 | 2.0 | 44.3 | 7.9 | 11.0 |
| D3 | 103 | 17.3 | 25.3 | 1.9 | 40.3 | 9.8 | 10.0 |

Notice kinds: Rathkit Pack hmr; esbuild change; rspack ok; Vite update; webpack ok; Parcel 2 update; Parcel 3 update. Without a development server, so not in these tables: rolldown, Rollup.

## Build plus type check (TypeScript workloads only; wall time and peak memory of every process of the setup)

500-module TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 728 | - | - | 237 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 67.6 | 792 | 724 | 50 | 233 |
| rolldown: rolldown, then `tsc --noEmit` | 155 | 895 | 739 | 182 | 239 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 499 | 1032 | 533 | 158 | 462 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 239 | 994 | 755 | 201 | 237 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 3876 | 4111 | 235 | 494 | 873 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 2322 | 2523 | 201 | 506 | 520 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 1290 | 2023 | 733 | 537 | 540 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 88.7 | 811 | 723 | 77 | 233 |

Preact app (TSX):

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 493 | - | - | 209 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 21.5 | 518 | 496 | 17 | 208 |
| rolldown: rolldown, then `tsc --noEmit` | 109 | 597 | 488 | 133 | 208 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 122 | 776 | 654 | 114 | 390 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 171 | 665 | 494 | 160 | 209 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 659 | 976 | 317 | 212 | 570 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 952 | 1088 | 136 | 308 | 317 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 584 | 1108 | 524 | 338 | 338 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 28.1 | 535 | 507 | 44 | 207 |

Monorepo-style TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 746 | - | - | 231 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 37.0 | 765 | 728 | 32 | 232 |
| rolldown: rolldown, then `tsc --noEmit` | 116 | 864 | 748 | 155 | 232 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 250 | 1034 | 784 | 145 | 446 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 188 | 929 | 741 | 184 | 231 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 2620 | 2863 | 244 | 466 | 861 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 1409 | 1679 | 270 | 378 | 402 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 859 | 1623 | 765 | 493 | 488 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 45.0 | 789 | 744 | 65 | 232 |
<!-- pack-bench:end -->

(*) rspack and Rollup fold the whole generated 1,500-module app to a constant (1 and 3 KB) and Rollup does the same to the Solid app
(3 KB), so their sizes say nothing about those cases. Parcel 3's 3 KB for lodash-es is also not a comparable result: nothing the
entry re-exports was kept. Different tools pick different package entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Time and memory:** Rathkit Pack took the least memory in all 45 cases (15 workloads, three modes) against every tool, and less
time than rolldown, rspack, Vite, webpack and Rollup in all 45. Against esbuild it took less time in 42 of the 45 cases; esbuild was
faster in three, by 1.2 ms or less (Zod minified 21.4 against 20.2 ms, Zod minified with a map 24.4 against 23.6 ms, D3 minified with a
map 28.6 against 27.3 ms). Parcel 3 took less time in 42 cases and was faster in the three lodash-es cases, where it kept none of the
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
6990 KB, against 126 KB for Rathkit Pack). Parcel 3 is an alpha. Against Parcel 2 it was between 2.6x and 40.6x faster on the inputs both
built, apart from the lodash-es cases where it kept nothing (124x to 158x), and used less memory in all 42; this was not close to the 100x
its announcement gives as the best case, which was measured on other, larger inputs.

**Rebuild and HMR:** in watch mode Rathkit Pack's warm rebuild is not the fastest, and the roadmap goal here is a measurement, not a win.
Plain, Parcel 3 had the lowest rebuild time on 14 of the 15 workloads and Parcel 2 on the other (the TypeScript compiler); minified,
Parcel 3 on 14 and esbuild on one. Pack's rebuild is 45 to 184 ms plain and 49 to 240 ms minified, and it was slower than esbuild's
on 13 of the 15 workloads in each mode (esbuild 15 to 477 ms; the 1,500-module app is where esbuild is slowest). Part of Pack's time
is how it notices a change: it polls the files its build read every 80 ms and waits 40 ms more, so up to 120 ms of each figure is
that; the other tools use file events, except esbuild, which scans in rounds. Rebuilds of webpack and Rollup minified take up to 18 and
24 s on the TypeScript compiler, against 240 ms for Pack. HMR latency in Pack's development server is 85 to 318 ms (the 1,500-module
app), the slowest of the tools that have one apart from esbuild's live reload on that app (469 ms; it is not HMR, it signals a change);
rspack is 21 to 47 ms, webpack 36 to 202 ms, Vite 1.4 to 15 ms, Parcel 2 7 to 11 ms and Parcel 3 2.4 to 37 ms. Pack's development
server started fastest on 11 of the 14 served workloads; Parcel 3 was fastest on lodash-es, rxjs and the
1,500-module app. Vite does not bundle, so its start covers the modules of the page only because the measurement asks for all of
them. None of this was run in a browser, and a few cases failed (Parcel 2's development server was run without its `packageExports`
option, so it could not resolve packages that only have `exports`; webpack and rspack reported errors on the Svelte app's HMR edits).

**Build plus type check:** Rathkit Pack has no type checker, so it has no figure in that table and nothing is invented for it; the cost
of `tsc --noEmit` alone is 0.5 to 0.75 s on these three workloads and 230 MB. Where a checker runs in the build, the check adds 0.14 to 0.3 s
for webpack and Rollup (the build is slow enough to hide most of it), 0.5 to 0.8 s for rspack and 0.5 to 0.77 s as a separate step. The
issue for this table (3.13) stays open until Pack has a checker of its own to put in the column.
