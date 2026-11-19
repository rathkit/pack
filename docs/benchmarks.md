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
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.58.1-microsoft-standard-WSL2; WSL2, Docker Desktop 29.9.3, laptop on AC. The machine was idle: the load average was 0.67, 0.23, 0.12 before the run and 1.48, 1.5, 1.64 after it. Node 26.13.0; Rathkit Pack 0.3.0-beta.0 at commit `9f2a929` (release build, sha256 `b0507c2ab6a2`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha), webpack-dev-server 6.1.0, @rspack/dev-server 2.2.2, TypeScript 5.9.3 (tsc), ts-checker-rspack-plugin 1.7.2, fork-ts-checker-webpack-plugin 9.1.0. Run on 2026-11-19 in the pack-bench container, bench commit `856bde4`, image `4df68dbdd905`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `856bde4`, build Pack at commit `9f2a929` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261119T125801Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --rebuild --rebuild-modes plain,minify --dev --typecheck`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 175 | 353 | 681 | 335 | 849 | 1241 | 8537 | 2182 | 577 |
| three.js | 38 | 61 | 137 | 143 | 229 | 582 | 2387 | 710 | 89 |
| lodash-es | 18 | 34 | 108 | 122 | 182 | 728 | 3577 | 1057 | 7 |
| React + react-dom/server | 11 | 18 | 106 | 95 | 167 | 381 | 2153 | 599 | 37 |
| rxjs | 10 | 22 | 103 | 104 | 161 | 544 | 2864 | 740 | 24 |
| 1,500-module app | 83 | 492 | 212 | 580 | 302 | 1843 | 5561 | failed | 187 |
| 500-module TypeScript app | 24 | 54 | 147 | 326 | 217 | 925 | 2224 | 1196 | 69 |
| Preact app (TSX) | 6 | 9 | 85 | 93 | 149 | 339 | 1890 | 537 | 14 |
| Vue app (render functions) | 17 | 42 | 151 | 122 | 180 | 612 | 2211 | 680 | 57 |
| Solid app (no JSX) | 9 | 15 | 90 | 94 | 157 | 363 | 1863 | 544 | 22 |
| Svelte app (precompiled) | 9 | 15 | 97 | 107 | 162 | 461 | 2212 | 606 | 20 |
| Monorepo-style TypeScript app | 12 | 21 | 95 | 141 | 163 | 641 | 1986 | 807 | 25 |
| date-fns | 12 | 27 | 97 | 111 | 162 | 475 | 4297 | 815 | 34 |
| Zod | 12 | 19 | 104 | 109 | 171 | 522 | 2318 | 608 | 26 |
| D3 | 14 | 23 | 106 | 104 | 174 | 496 | 2176 | 594 | 29 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 257 | 364 | 811 | 1689 | 1024 | 18436 | 27880 | 3630 | 1395 |
| three.js | 46 | 68 | 143 | 297 | 237 | 2476 | 3528 | 844 | 129 |
| lodash-es | 21 | 35 | 117 | 160 | 190 | 1438 | 4060 | 1108 | 7 |
| React + react-dom/server | 16 | 19 | 106 | 169 | 175 | 1419 | 2892 | 771 | 63 |
| rxjs | 13 | 22 | 113 | 152 | 162 | 1433 | 3116 | 794 | 27 |
| 1,500-module app | 88 | 489 | 217 | 793 | 313 | 4067 | 5603 | failed | 176 |
| 500-module TypeScript app | 28 | 55 | 146 | 471 | 219 | 3826 | 2952 | 1238 | 74 |
| Preact app (TSX) | 7 | 9 | 87 | 108 | 150 | 627 | 2083 | 554 | 16 |
| Vue app (render functions) | 21 | 45 | 166 | 348 | 194 | 3193 | 2739 | 877 | 89 |
| Solid app (no JSX) | 11 | 16 | 92 | 126 | 158 | 893 | 1976 | 575 | 31 |
| Svelte app (precompiled) | 11 | 15 | 96 | 131 | 161 | 935 | 2429 | 645 | 23 |
| Monorepo-style TypeScript app | 15 | 21 | 96 | 229 | 163 | 2572 | 2309 | 850 | 27 |
| date-fns | 15 | 27 | 100 | 125 | 170 | 771 | 4503 | 797 | 36 |
| Zod | 15 | 20 | 107 | 142 | 172 | 1239 | 2790 | 677 | 37 |
| D3 | 17 | 24 | 110 | 145 | 177 | 1280 | 2635 | 649 | 47 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 316 | 529 | 915 | 1971 | 1106 | 19182 | 28854 | 4739 | 1494 |
| three.js | 57 | 83 | 155 | 338 | 250 | 2644 | 3625 | 1043 | 144 |
| lodash-es | 29 | 37 | 120 | 169 | 191 | 1507 | 4120 | 1149 | 8 |
| React + react-dom/server | 20 | 24 | 116 | 186 | 177 | 1501 | 2969 | 854 | 69 |
| rxjs | 15 | 28 | 116 | 165 | 162 | 1484 | 3115 | 814 | 29 |
| 1,500-module app | 102 | 509 | 225 | 808 | 321 | 4215 | 5602 | failed | 181 |
| 500-module TypeScript app | 34 | 58 | 150 | 518 | 224 | 3856 | 3041 | 1418 | 77 |
| Preact app (TSX) | 9 | 12 | 87 | 108 | 154 | 653 | 2101 | 592 | 18 |
| Vue app (render functions) | 25 | 88 | 184 | 389 | 196 | 3364 | 2800 | 1057 | 101 |
| Solid app (no JSX) | 14 | 17 | 93 | 131 | 163 | 943 | 1984 | 600 | 35 |
| Svelte app (precompiled) | 12 | 16 | 99 | 135 | 165 | 971 | 2452 | 666 | 24 |
| Monorepo-style TypeScript app | 17 | 23 | 98 | 250 | 164 | 2662 | 2333 | 884 | 28 |
| date-fns | 16 | 29 | 103 | 128 | 167 | 794 | 4552 | 879 | 39 |
| Zod | 20 | 21 | 109 | 154 | 178 | 1288 | 2818 | 723 | 40 |
| D3 | 20 | 26 | 115 | 153 | 180 | 1328 | 2694 | 705 | 50 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 2.0x, rolldown 7.3x, rspack 8.1x, Vite 11.4x, webpack 34.3x, Rollup 156.9x, Parcel 2 48.7x, Parcel 3 2.3x;
minified, esbuild 1.7x, rolldown 6.1x, rspack 10.5x, Vite 9.5x, webpack 84.8x, Rollup 158.1x, Parcel 2 43.7x, Parcel 3 2.3x;
minified with a map, esbuild 1.6x, rolldown 5.2x, rspack 9.3x, Vite 8.0x, webpack 72.8x, Rollup 132.3x, Parcel 2 39.7x, Parcel 3 2.1x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 192 | 265 | 532 | 423 | 464 | 1777 | 3174 | 1398 | 373 |
| three.js | 42 | 60 | 205 | 193 | 226 | 490 | 709 | 566 | 149 |
| lodash-es | 21 | 56 | 237 | 144 | 265 | 349 | 568 | 534 | 27 |
| React + react-dom/server | 21 | 29 | 189 | 131 | 213 | 325 | 595 | 504 | 182 |
| rxjs | 12 | 49 | 224 | 131 | 209 | 360 | 487 | 483 | 90 |
| 1,500-module app | 93 | 380 | 468 | 350 | 496 | 1051 | 844 | failed | 242 |
| 500-module TypeScript app | 26 | 55 | 194 | 181 | 205 | 532 | 596 | 571 | 81 |
| Preact app (TSX) | 9 | 20 | 143 | 113 | 168 | 211 | 435 | 348 | 58 |
| Vue app (render functions) | 18 | 88 | 319 | 178 | 208 | 541 | 621 | 584 | 200 |
| Solid app (no JSX) | 12 | 22 | 160 | 119 | 190 | 241 | 414 | 378 | 87 |
| Svelte app (precompiled) | 11 | 27 | 206 | 123 | 224 | 235 | 465 | 420 | 91 |
| Monorepo-style TypeScript app | 13 | 34 | 162 | 154 | 198 | 463 | 507 | 524 | 71 |
| date-fns | 13 | 41 | 196 | 124 | 216 | 210 | 769 | 514 | 77 |
| Zod | 20 | 42 | 252 | 133 | 280 | 318 | 546 | 452 | 145 |
| D3 | 19 | 41 | 266 | 132 | 299 | 304 | 555 | 453 | 157 |

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
| TypeScript compiler | 62.7 | 113 | 579 | 263 | 718 | 775 | 4749 | 59.5 | 108 |
| three.js | 12.5 | 58.1 | 64.5 | 76.0 | 99.2 | 194 | 459 | 23.4 | 18.3 |
| lodash-es | 14.3 | 41.2 | 35.6 | 63.1 | 49.0 | 238 | 228 | 48.3 | 2.8 |
| React + react-dom/server | 5.7 | 16.9 | 34.9 | 30.2 | 41.7 | 85.7 | 191 | 16.5 | 11.5 |
| rxjs | 6.9 | 23.1 | 30.8 | 41.1 | 26.7 | 149 | 114 | 21.0 | 5.9 |
| 1,500-module app | 42.5 | 482 | 141 | 530 | 174 | 1124 | 1300 | failed | 32.4 |
| 500-module TypeScript app | 17.5 | 53.0 | 75.1 | 265 | 92.4 | 455 | 572 | 29.6 | 8.6 |
| Preact app (TSX) | 2.4 | 14.3 | 15.5 | 29.5 | 21.1 | 54.6 | 324 | 12.6 | 2.7 |
| Vue app (render functions) | 8.1 | 35.7 | 73.9 | 55.4 | 47.6 | 190 | 232 | 20.9 | 15.3 |
| Solid app (no JSX) | 4.7 | 19.9 | 22.2 | 32.9 | 27.9 | 71.8 | 43.9 | 13.8 | 8.3 |
| Svelte app (precompiled) | 4.6 | 20.2 | 21.2 | 42.0 | 29.7 | 112 | 99.5 | 19.9 | 5.4 |
| Monorepo-style TypeScript app | 7.8 | 25.1 | 26.5 | 82.5 | 29.5 | 223 | 416 | 21.1 | 6.0 |
| date-fns | 8.6 | 28.7 | 27.8 | 50.6 | 31.5 | 101 | 100 | 19.5 | 6.2 |
| Zod | 9.0 | 21.5 | 30.2 | 43.4 | 37.1 | 139 | 228 | 20.4 | 10.4 |
| D3 | 9.2 | 19.7 | 31.6 | 39.7 | 38.0 | 116 | 232 | 17.2 | 9.7 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 65.0 | 77.2 | 722 | 1674 | 873 | 18210 | 24153 | n/a | 116 |
| three.js | 16.6 | 65.2 | 73.8 | 229 | 109 | 1978 | 1537 | n/a | 19.6 |
| lodash-es | 17.2 | 41.4 | 40.4 | 98.4 | 47.6 | 869 | 706 | n/a | 2.6 |
| React + react-dom/server | 9.4 | 15.0 | 39.8 | 101 | 45.4 | 1061 | 864 | n/a | 12.5 |
| rxjs | 8.9 | 24.3 | 38.2 | 93.1 | 29.8 | 930 | 305 | n/a | 6.0 |
| 1,500-module app | 46.4 | 485 | 142 | 713 | 178 | 3283 | 1404 | n/a | 30.8 |
| 500-module TypeScript app | 21.5 | 54.0 | 76.6 | 409 | 96.7 | 3272 | 1286 | n/a | 9.2 |
| Preact app (TSX) | 5.4 | 14.3 | 17.3 | 40.9 | 21.3 | 298 | 498 | n/a | 4.6 |
| Vue app (render functions) | 9.6 | 40.7 | 96.6 | 282 | 59.8 | 2725 | 725 | n/a | 15.1 |
| Solid app (no JSX) | 7.6 | 21.6 | 25.1 | 59.0 | 29.8 | 534 | 124 | n/a | 6.2 |
| Svelte app (precompiled) | 7.2 | 21.7 | 23.6 | 62.0 | 30.0 | 521 | 289 | n/a | 5.7 |
| Monorepo-style TypeScript app | 9.3 | 25.9 | 25.8 | 174 | 31.2 | 2088 | 667 | n/a | 8.2 |
| date-fns | 9.5 | 25.9 | 25.3 | 60.7 | 36.1 | 346 | 294 | n/a | 5.0 |
| Zod | 9.6 | 27.0 | 31.4 | 79.9 | 40.0 | 801 | 657 | n/a | 10.0 |
| D3 | 9.7 | 22.0 | 35.5 | 81.0 | 41.8 | 837 | 661 | n/a | 7.7 |

Each tool runs in its own watch mode on a writable copy of the project, the first build is waited for, then one file of the project is changed 7 times (each change appends one statement), and the time from writing the file to the output file holding the statement is taken; the median of the 7 is shown.

## Development server: start and HMR latency (median ms, lower is better)

Start, until the page and every module it imports have been answered:

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 75.0 | 121 | 213 | 190 | 684 | 850 | 101 |
| lodash-es | 44.3 | 99.8 | 200 | 196 | 855 | 1164 | 11.0 |
| React + react-dom/server | 31.0 | 81.9 | 169 | 265 | 526 | 697 | 41.5 |
| rxjs | 30.8 | 81.7 | 155 | 258 | 544 | 821 | 22.2 |
| 1,500-module app | 316 | 570 | 468 | 1499 | 1779 | failed | 221 |
| 500-module TypeScript app | 47.1 | 118 | 343 | 705 | 1084 | 1312 | 74.1 |
| Preact app (TSX) | 17.9 | 68.1 | 153 | 263 | 501 | 633 | 21.0 |
| Vue app (render functions) | 31.2 | 89.7 | 172 | 261 | 579 | 684 | 50.7 |
| Solid app (no JSX) | 19.3 | 68.4 | 154 | 253 | 478 | 633 | 22.3 |
| Svelte app (precompiled) | 30.3 | 79.5 | 164 | 262 | 625 | failed | failed |
| Monorepo-style TypeScript app | 28.9 | 84.6 | 202 | 425 | 783 | 898 | 30.5 |
| date-fns | 30.5 | 90.6 | 172 | 203 | 658 | 915 | 42.0 |
| Zod | 42.0 | 84.2 | 172 | 204 | 643 | 756 | 44.0 |
| D3 | 41.9 | 92.1 | 163 | 206 | 656 | 694 | 45.9 |

HMR latency, from writing a change to a module that accepts it to the update notice (the kind of notice is in the second table):

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 19.6 | 56.3 | 30.1 | 2.0 | 39.7 | 6.8 | 32.8 |
| lodash-es | 20.5 | 36.8 | 28.9 | 2.0 | 71.4 | 10.0 | 1.9 |
| React + react-dom/server | 8.8 | 13.3 | 23.4 | 2.1 | 36.5 | 6.6 | 14.0 |
| rxjs | 10.2 | 18.8 | 21.3 | 2.1 | 38.9 | 8.5 | 4.2 |
| 1,500-module app | 135 | 464 | 50.1 | 3.0 | 197 | none | 39.9 |
| 500-module TypeScript app | 19.4 | 53.3 | 28.8 | 14.8 | 69.7 | 9.2 | 8.7 |
| Preact app (TSX) | 3.4 | 12.6 | 21.1 | 1.6 | 37.2 | 7.1 | 4.2 |
| Vue app (render functions) | 9.3 | 15.1 | 22.8 | 3.3 | 39.8 | 7.2 | 11.0 |
| Solid app (no JSX) | 3.2 | 14.1 | 20.1 | 1.4 | 36.8 | 8.9 | 5.6 |
| Svelte app (precompiled) | 10.2 | 17.7 | 22.6 | 1.5 | 40.8 | none | none |
| Monorepo-style TypeScript app | 9.7 | 25.0 | 28.7 | 3.9 | 62.1 | 9.6 | 6.0 |
| date-fns | 10.3 | 25.1 | 22.6 | 2.3 | 48.2 | 9.5 | 5.9 |
| Zod | 15.5 | 30.7 | 22.8 | 5.1 | 46.6 | 10.1 | 10.0 |
| D3 | 13.9 | 19.5 | 22.1 | 5.4 | 40.7 | 9.0 | 10.2 |

Notice kinds: Rathkit Pack hmr; esbuild change; rspack ok/warnings; Vite update; webpack ok/warnings; Parcel 2 update; Parcel 3 update. Without a development server: rolldown (no development server (the rolldown command line has none)); Rollup (no development server).

Failed: 1,500-module app on Parcel 2 (Parcel 2 stops with an internal assertion error in its bundler); Svelte app (precompiled) on Parcel 2 (Parcel 2 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container); Svelte app (precompiled) on Parcel 3 (Parcel 3 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container).

Not measured: TypeScript compiler (the TypeScript compiler is a library for Node with no page to serve: it needs fs, path, os and other Node modules, which each tool would have to be set up to stub differently).

## Build plus type check (TypeScript workloads only; wall time and peak memory of every process of the setup)

500-module TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 731 | - | - | 240 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 66.8 | 786 | 719 | 51 | 238 |
| rolldown: rolldown, then `tsc --noEmit` | 157 | 887 | 729 | 183 | 234 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 490 | 1035 | 545 | 157 | 463 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 237 | 964 | 727 | 201 | 235 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 3891 | 4129 | 238 | 495 | 866 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 2315 | 2501 | 186 | 507 | 517 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 1273 | 1990 | 717 | 542 | 540 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 89.0 | 814 | 725 | 79 | 241 |

Preact app (TSX):

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 496 | - | - | 208 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 22.8 | 514 | 491 | 17 | 208 |
| rolldown: rolldown, then `tsc --noEmit` | 96.5 | 590 | 494 | 133 | 209 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 120 | 787 | 668 | 114 | 391 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 163 | 659 | 496 | 156 | 208 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 692 | 981 | 289 | 215 | 564 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 941 | 1078 | 138 | 309 | 319 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 583 | 1091 | 508 | 336 | 337 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 28.8 | 525 | 496 | 44 | 208 |

Monorepo-style TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 731 | - | - | 232 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 39.6 | 771 | 732 | 32 | 231 |
| rolldown: rolldown, then `tsc --noEmit` | 109 | 846 | 737 | 157 | 232 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 250 | 1034 | 784 | 145 | 447 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 195 | 925 | 730 | 186 | 231 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 2620 | 2845 | 226 | 466 | 857 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 1383 | 1672 | 290 | 380 | 401 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 863 | 1612 | 748 | 491 | 494 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 46.7 | 790 | 743 | 67 | 231 |
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
minified): 2.3 to 63.9 ms plain and 5.0 to 63.9 ms minified, against 14 to 475 ms plain and 15 to 487 ms minified for esbuild. It is
faster than rolldown, rspack, Vite, webpack and Rollup in all 30. Parcel 2 is faster only on the TypeScript compiler plain; Parcel 3
is faster in 15 of the 30, mostly on the smaller inputs (lodash-es, rxjs, the generated apps, date-fns, the monorepo-style app and the
minified Preact, Solid and Svelte apps). The lowest rebuild time plain was Pack's on 8 of the 15 workloads, Parcel 3's on 6 and Parcel
2's on one; minified, Pack's on 6 and Parcel 3's on 9. On the 1,500-module app Pack takes 41 and 44 ms and esbuild about 475 and 487 ms.
Rebuilds of webpack and Rollup minified take 18 and 24 s on the TypeScript compiler, against 64 ms for Pack. On Linux Pack hears of a change
through the kernel's file events; on macOS and Windows it still polls, so these figures say nothing about those systems.

**HMR:** latency in Pack's development server is 3.2 to 135 ms (the 1,500-module app). It is lower than esbuild's in all 14 served
workloads (esbuild 11 to 478 ms; esbuild's is a live-reload signal, not HMR), than webpack's in all 14 (36 to 199 ms) and than rspack's
in 13 of 14 (21 to 44 ms; rspack is faster on the 1,500-module app). It is higher than Vite's in all 14 (1.3 to 14.5 ms), than Parcel 2's in
10 of the 12 it has figures for (7.0 to 10.7 ms) and than Parcel 3's in 8 of the 13 (2.0 to 42 ms). Pack's development server started
fastest on 11 of the 14 served workloads; Parcel 3 was fastest on lodash-es, rxjs and the 1,500-module app. Vite does not bundle, so its start
covers the modules of the page only because the measurement asks for all of them. None of this was run in a browser. Parcel 2 has no figure
for the 1,500-module app and the Svelte app, and Parcel 3 none for the Svelte app. The TypeScript compiler has no development-server figure for any tool, for the reason in the method.

**Build plus type check:** Rathkit Pack's own checker (`rkp check`, stage 1) is not measured in this table, so its row has no figure and
nothing is invented for it. The cost of `tsc --noEmit` alone is 0.49 to 0.75 s on these three workloads and 207 to 236 MB. Where a checker runs
in the build, the check adds time on top of the build for every tool; the rows show how much.
