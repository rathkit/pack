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
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.58.1-microsoft-standard-WSL2; WSL2, Docker Desktop, laptop on AC. The machine was idle: the load average was 0.75, 0.65, 0.74 before the run and 1.16, 1.24, 1.32 after it. Node 26.13.0; Rathkit Pack 0.2.0-beta.0 at commit `ff0f3d1` (release build, sha256 `0851f6a430cb`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha), webpack-dev-server 6.1.0, @rspack/dev-server 2.2.2, TypeScript 5.9.3 (tsc), ts-checker-rspack-plugin 1.7.2, fork-ts-checker-webpack-plugin 9.1.0. Run on 2026-11-16 in the pack-bench container, bench commit `33fa229`, image `3b85302e8379`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `33fa229`, build Pack at commit `ff0f3d1` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261116T191554Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --rebuild --rebuild-modes plain,minify --dev --typecheck`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 196 | 392 | 713 | 335 | 864 | 1272 | 8598 | 2241 | 584 |
| three.js | 40 | 62 | 135 | 146 | 235 | 587 | 2398 | 718 | 89 |
| lodash-es | 19 | 35 | 111 | 149 | 186 | 741 | 3620 | 1073 | 8 |
| React + react-dom/server | 12 | 19 | 105 | 100 | 174 | 388 | 2186 | 607 | 38 |
| rxjs | 12 | 22 | 106 | 105 | 165 | 554 | 2936 | 786 | 25 |
| 1,500-module app | 91 | 484 | 225 | 582 | 315 | 1934 | 5556 | failed | 193 |
| 500-module TypeScript app | 26 | 54 | 143 | 330 | 222 | 931 | 2217 | 1197 | 72 |
| Preact app (TSX) | 6 | 10 | 85 | 96 | 151 | 341 | 1897 | 558 | 15 |
| Vue app (render functions) | 17 | 44 | 147 | 123 | 180 | 617 | 2209 | 682 | 55 |
| Solid app (no JSX) | 9 | 16 | 95 | 99 | 157 | 372 | 1854 | 543 | 25 |
| Svelte app (precompiled) | 9 | 15 | 97 | 108 | 164 | 466 | 2195 | 626 | 22 |
| Monorepo-style TypeScript app | 11 | 22 | 93 | 143 | 165 | 646 | 2001 | 816 | 25 |
| date-fns | 12 | 28 | 101 | 116 | 168 | 482 | 4296 | 786 | 36 |
| Zod | 14 | 19 | 106 | 110 | 171 | 526 | 2301 | 631 | 28 |
| D3 | 14 | 24 | 110 | 108 | 176 | 506 | 2180 | 595 | 29 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 255 | 372 | 811 | 1751 | 1023 | 18592 | 27944 | 3669 | 1393 |
| three.js | 47 | 71 | 148 | 300 | 246 | 2512 | 3591 | 877 | 132 |
| lodash-es | 23 | 35 | 122 | 162 | 185 | 1463 | 4113 | 1125 | 8 |
| React + react-dom/server | 17 | 20 | 109 | 173 | 179 | 1423 | 2939 | 776 | 63 |
| rxjs | 13 | 23 | 123 | 160 | 167 | 1469 | 3190 | 828 | 28 |
| 1,500-module app | 90 | 490 | 215 | 783 | 311 | 4072 | 5665 | failed | 183 |
| 500-module TypeScript app | 31 | 55 | 142 | 477 | 227 | 3876 | 2961 | 1251 | 75 |
| Preact app (TSX) | 7 | 10 | 92 | 112 | 151 | 633 | 2096 | 561 | 20 |
| Vue app (render functions) | 23 | 46 | 171 | 348 | 192 | 3184 | 2748 | 896 | 92 |
| Solid app (no JSX) | 11 | 16 | 93 | 126 | 164 | 897 | 1967 | 580 | 30 |
| Svelte app (precompiled) | 10 | 17 | 98 | 132 | 166 | 935 | 2453 | 642 | 24 |
| Monorepo-style TypeScript app | 15 | 22 | 100 | 234 | 167 | 2586 | 2296 | 838 | 27 |
| date-fns | 14 | 27 | 101 | 127 | 166 | 761 | 4513 | 805 | 36 |
| Zod | 17 | 21 | 108 | 146 | 178 | 1257 | 2794 | 689 | 37 |
| D3 | 19 | 25 | 116 | 151 | 180 | 1285 | 2650 | 677 | 48 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 316 | 535 | 914 | 1997 | 1107 | 19248 | 28919 | 4772 | 1496 |
| three.js | 56 | 87 | 162 | 348 | 256 | 2698 | 3674 | 1047 | 146 |
| lodash-es | 30 | 39 | 122 | 177 | 193 | 1522 | 4143 | 1171 | 9 |
| React + react-dom/server | 21 | 26 | 117 | 187 | 183 | 1518 | 2990 | 898 | 72 |
| rxjs | 15 | 29 | 118 | 169 | 166 | 1499 | 3203 | 930 | 35 |
| 1,500-module app | 113 | 500 | 223 | 799 | 317 | 4157 | 5620 | failed | 186 |
| 500-module TypeScript app | 37 | 58 | 148 | 525 | 231 | 3911 | 3027 | 1410 | 77 |
| Preact app (TSX) | 9 | 13 | 90 | 116 | 154 | 659 | 2116 | 598 | 19 |
| Vue app (render functions) | 25 | 89 | 190 | 394 | 200 | 3369 | 2860 | 1073 | 101 |
| Solid app (no JSX) | 13 | 18 | 96 | 129 | 166 | 939 | 2060 | 612 | 35 |
| Svelte app (precompiled) | 12 | 17 | 102 | 137 | 169 | 978 | 2460 | 678 | 25 |
| Monorepo-style TypeScript app | 17 | 24 | 99 | 257 | 170 | 2635 | 2320 | 875 | 30 |
| date-fns | 15 | 30 | 104 | 132 | 172 | 803 | 4514 | 805 | 38 |
| Zod | 21 | 22 | 119 | 158 | 181 | 1298 | 2826 | 734 | 44 |
| D3 | 22 | 27 | 120 | 160 | 183 | 1329 | 2694 | 732 | 49 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 2.0x, rolldown 7.0x, rspack 8.0x, Vite 11.0x, webpack 33.1x, Rollup 149.4x, Parcel 2 47.1x, Parcel 3 2.2x;
minified, esbuild 1.6x, rolldown 6.0x, rspack 10.3x, Vite 9.2x, webpack 81.9x, Rollup 152.6x, Parcel 2 42.5x, Parcel 3 2.3x;
minified with a map, esbuild 1.7x, rolldown 5.2x, rspack 9.3x, Vite 7.9x, webpack 71.7x, Rollup 130.6x, Parcel 2 39.7x, Parcel 3 2.1x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 194 | 265 | 502 | 426 | 476 | 1778 | 3196 | 1378 | 372 |
| three.js | 43 | 60 | 205 | 187 | 234 | 490 | 711 | 566 | 142 |
| lodash-es | 23 | 57 | 235 | 145 | 262 | 351 | 568 | 534 | 27 |
| React + react-dom/server | 21 | 29 | 190 | 130 | 213 | 325 | 594 | 507 | 172 |
| rxjs | 12 | 49 | 230 | 130 | 206 | 339 | 488 | 485 | 87 |
| 1,500-module app | 93 | 380 | 472 | 354 | 497 | 1050 | 843 | failed | 240 |
| 500-module TypeScript app | 29 | 55 | 190 | 178 | 203 | 533 | 596 | 570 | 83 |
| Preact app (TSX) | 9 | 20 | 143 | 113 | 166 | 210 | 436 | 347 | 58 |
| Vue app (render functions) | 18 | 88 | 321 | 175 | 210 | 540 | 608 | 590 | 203 |
| Solid app (no JSX) | 12 | 22 | 159 | 119 | 194 | 242 | 414 | 377 | 79 |
| Svelte app (precompiled) | 11 | 27 | 200 | 122 | 226 | 235 | 464 | 422 | 91 |
| Monorepo-style TypeScript app | 14 | 34 | 160 | 154 | 190 | 464 | 506 | 524 | 71 |
| date-fns | 13 | 41 | 197 | 124 | 218 | 207 | 762 | 509 | 79 |
| Zod | 21 | 42 | 259 | 132 | 286 | 318 | 547 | 451 | 141 |
| D3 | 21 | 42 | 268 | 133 | 281 | 299 | 553 | 451 | 157 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 3468 / 976 | 3515 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 | 3467 / 959 | 3458 / 959 | 3406 / 965 | 3425 / 969 |
| three.js | 514 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 | 524 / 126 | 506 / 123 | 519 / 128 | 617 / 152 |
| lodash-es | 75 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 | 72 / 25 | 71 / 24 | 85 / 29 | 3 / 1 |
| React + react-dom/server | 227 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 | 226 / 68 | 227 / 67 | 583 / 175 | 585 / 176 |
| rxjs | 24 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 | 143 / 28 | 22 / 7 | 142 / 27 | 175 / 39 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 | 80 / 17 | 3 / 1 | failed | 406 / 67 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 | 541 / 32 | 185 / 21 | 173 / 30 | 271 / 29 |
| Preact app (TSX) | 22 / 9 | 23 / 9 | 23 / 9 | 25 / 10 | 30 / 11 | 25 / 10 | 22 / 9 | 23 / 9 | 28 / 11 |
| Vue app (render functions) | 94 / 36 | 639 / 206 | 630 / 200 | 620 / 197 | 114 / 37 | 631 / 198 | 86 / 33 | 630 / 200 | 635 / 201 |
| Solid app (no JSX) | 49 / 15 | 49 / 15 | 49 / 15 | 44 / 14 | 63 / 16 | 48 / 15 | 3 / 1 | 44 / 14 | 66 / 20 |
| Svelte app (precompiled) | 27 / 10 | 27 / 10 | 24 / 9 | 25 / 9 | 32 / 10 | 25 / 9 | 24 / 9 | 24 / 9 | 98 / 31 |
| Monorepo-style TypeScript app | 59 / 5 | 60 / 5 | 63 / 5 | 340 / 13 | 99 / 6 | 348 / 13 | 59 / 5 | 62 / 5 | 142 / 16 |
| date-fns | 27 / 9 | 28 / 9 | 27 / 8 | 26 / 8 | 40 / 10 | 27 / 8 | 26 / 8 | 26 / 8 | 47 / 13 |
| Zod | 99 / 28 | 99 / 29 | 96 / 26 | 89 / 25 | 120 / 29 | 94 / 26 | 92 / 26 | 141 / 37 | 501 / 102 |
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
| TypeScript compiler | 63.9 | 116 | 595 | 280 | 723 | 788 | 4796 | 63.0 | 111 |
| three.js | 10.4 | 59.8 | 66.7 | 75.4 | 97.3 | 217 | 478 | 24.0 | 19.6 |
| lodash-es | 13.4 | 40.1 | 36.4 | 64.4 | 52.3 | 241 | 231 | 49.0 | 2.8 |
| React + react-dom/server | 5.5 | 17.6 | 33.8 | 32.3 | 41.0 | 89.2 | 189 | 17.4 | 13.2 |
| rxjs | 6.1 | 23.0 | 33.0 | 40.0 | 26.7 | 139 | 108 | 19.8 | 6.0 |
| 1,500-module app | 40.6 | 475 | 143 | 537 | 175 | 1131 | 1312 | failed | 32.9 |
| 500-module TypeScript app | 17.2 | 51.3 | 74.8 | 269 | 94.3 | 460 | 563 | 29.5 | 8.6 |
| Preact app (TSX) | 2.3 | 14.4 | 16.7 | 29.3 | 19.5 | 54.5 | 322 | 13.0 | 4.1 |
| Vue app (render functions) | 8.5 | 40.6 | 75.6 | 57.0 | 48.3 | 190 | 228 | 22.2 | 15.5 |
| Solid app (no JSX) | 4.7 | 21.4 | 21.6 | 34.0 | 29.1 | 72.3 | 44.6 | 12.5 | 6.1 |
| Svelte app (precompiled) | 5.1 | 19.9 | 21.1 | 40.6 | 28.0 | 107 | 96.9 | 19.5 | 5.4 |
| Monorepo-style TypeScript app | 9.0 | 26.4 | 22.4 | 84.5 | 30.4 | 224 | 412 | 23.9 | 6.1 |
| date-fns | 7.2 | 25.9 | 24.0 | 49.4 | 30.2 | 105 | 101 | 19.3 | 5.3 |
| Zod | 9.3 | 30.0 | 29.0 | 43.9 | 37.8 | 155 | 232 | 20.9 | 10.7 |
| D3 | 7.9 | 20.3 | 30.1 | 37.7 | 38.9 | 113 | 228 | 17.1 | 9.2 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 63.9 | 75.7 | 732 | 1756 | 876 | 18498 | 24353 | n/a | 109 |
| three.js | 14.4 | 67.9 | 72.1 | 230 | 105 | 2039 | 1543 | n/a | 16.8 |
| lodash-es | 15.8 | 41.7 | 40.0 | 99.2 | 46.9 | 910 | 675 | n/a | 2.7 |
| React + react-dom/server | 9.6 | 14.7 | 39.6 | 104 | 49.1 | 1076 | 896 | n/a | 11.8 |
| rxjs | 8.2 | 23.0 | 39.9 | 89.3 | 27.3 | 937 | 313 | n/a | 7.3 |
| 1,500-module app | 44.1 | 487 | 143 | 714 | 176 | 3273 | 1361 | n/a | 36.1 |
| 500-module TypeScript app | 20.2 | 56.2 | 78.5 | 413 | 102 | 3288 | 1313 | n/a | 8.3 |
| Preact app (TSX) | 5.0 | 15.5 | 18.2 | 40.6 | 22.1 | 303 | 489 | n/a | 3.3 |
| Vue app (render functions) | 9.7 | 37.1 | 99.1 | 281 | 59.7 | 2746 | 721 | n/a | 15.6 |
| Solid app (no JSX) | 8.3 | 22.9 | 22.9 | 59.5 | 29.2 | 543 | 124 | n/a | 6.2 |
| Svelte app (precompiled) | 8.6 | 20.5 | 23.5 | 65.0 | 29.8 | 523 | 295 | n/a | 5.4 |
| Monorepo-style TypeScript app | 9.2 | 26.6 | 26.4 | 174 | 33.8 | 2101 | 682 | n/a | 8.0 |
| date-fns | 8.9 | 29.5 | 25.0 | 61.4 | 31.9 | 355 | 291 | n/a | 6.3 |
| Zod | 9.8 | 29.6 | 30.6 | 79.6 | 39.4 | 796 | 690 | n/a | 10.3 |
| D3 | 9.7 | 21.1 | 39.5 | 80.0 | 39.7 | 831 | 670 | n/a | 9.9 |

Each tool runs in its own watch mode on a writable copy of the project, the first build is waited for, then one file of the project is changed 7 times (each change appends one statement), and the time from writing the file to the output file holding the statement is taken; the median of the 7 is shown.

## Development server: start and HMR latency (median ms, lower is better)

Start, until the page and every module it imports have been answered:

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 69.8 | 123 | 216 | 191 | 692 | 868 | 109 |
| lodash-es | 48.8 | 99.6 | 201 | 207 | 869 | 1198 | 10.7 |
| React + react-dom/server | 31.5 | 82.4 | 176 | 270 | 542 | 704 | 44.5 |
| rxjs | 37.7 | 81.4 | 160 | 263 | 547 | 828 | 26.7 |
| 1,500-module app | 309 | 574 | 482 | 1508 | 1767 | failed | 223 |
| 500-module TypeScript app | 50.2 | 120 | 347 | 718 | 1110 | 1315 | 79.6 |
| Preact app (TSX) | 18.8 | 68.7 | 158 | 264 | 507 | 650 | 19.5 |
| Vue app (render functions) | 32.2 | 89.8 | 175 | 261 | 588 | 675 | 52.0 |
| Solid app (no JSX) | 19.5 | 69.8 | 157 | 253 | 483 | 635 | 20.4 |
| Svelte app (precompiled) | 30.8 | 81.2 | 173 | 268 | 623 | failed | failed |
| Monorepo-style TypeScript app | 30.1 | 89.1 | 207 | 428 | 783 | 917 | 32.0 |
| date-fns | 30.5 | 92.8 | 179 | 206 | 656 | 913 | 42.3 |
| Zod | 42.9 | 88.2 | 172 | 206 | 650 | 860 | 45.1 |
| D3 | 43.0 | 93.2 | 172 | 213 | 661 | 707 | 46.0 |

HMR latency, from writing a change to a module that accepts it to the update notice (the kind of notice is in the second table):

| | Rathkit Pack | esbuild | rspack | Vite | webpack | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| three.js | 19.3 | 57.0 | 28.3 | 1.9 | 40.2 | 7.0 | 32.6 |
| lodash-es | 19.2 | 39.2 | 30.2 | 2.1 | 72.9 | 10.7 | 2.0 |
| React + react-dom/server | 7.9 | 15.4 | 23.1 | 2.1 | 35.8 | 7.5 | 12.8 |
| rxjs | 9.5 | 18.2 | 23.2 | 1.9 | 39.3 | 8.9 | 4.6 |
| 1,500-module app | 135 | 478 | 43.9 | 2.9 | 199 | none | 42.1 |
| 500-module TypeScript app | 18.7 | 53.8 | 29.0 | 14.5 | 70.9 | 10.1 | 8.9 |
| Preact app (TSX) | 3.2 | 11.3 | 21.5 | 2.9 | 36.6 | 7.3 | 4.0 |
| Vue app (render functions) | 9.1 | 11.7 | 22.4 | 1.3 | 40.2 | 7.2 | 11.3 |
| Solid app (no JSX) | 3.5 | 12.7 | 21.4 | 1.4 | 37.5 | 8.4 | 6.2 |
| Svelte app (precompiled) | 7.6 | 18.3 | 25.0 | 1.7 | 42.1 | none | none |
| Monorepo-style TypeScript app | 9.4 | 23.4 | 28.7 | 3.7 | 64.4 | 9.3 | 6.0 |
| date-fns | 10.1 | 27.3 | 24.1 | 4.7 | 49.1 | 8.5 | 5.7 |
| Zod | 13.2 | 20.9 | 23.9 | 1.9 | 45.3 | 10.4 | 10.6 |
| D3 | 12.7 | 18.8 | 22.7 | 1.9 | 39.5 | 8.0 | 10.4 |

Notice kinds: Rathkit Pack hmr; esbuild change; rspack ok/warnings; Vite update; webpack ok/warnings; Parcel 2 update; Parcel 3 update. Without a development server: rolldown (no development server (the rolldown command line has none)); Rollup (no development server).

Failed: 1,500-module app on Parcel 2 (Parcel 2 stops with an internal assertion error in its bundler); Svelte app (precompiled) on Parcel 2 (Parcel 2 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container); Svelte app (precompiled) on Parcel 3 (Parcel 3 needs the `buffer` polyfill package for a Node global that Svelte's server runtime uses, and package installs are off in the container).

Not measured: TypeScript compiler (the TypeScript compiler is a library for Node with no page to serve: it needs fs, path, os and other Node modules, which each tool would have to be set up to stub differently).

## Build plus type check (TypeScript workloads only; wall time and peak memory of every process of the setup)

500-module TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 720 | - | - | 236 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 68.5 | 799 | 730 | 51 | 235 |
| rolldown: rolldown, then `tsc --noEmit` | 153 | 885 | 732 | 185 | 239 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 493 | 1035 | 542 | 158 | 465 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 241 | 979 | 738 | 199 | 236 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 3914 | 4067 | 153 | 495 | 869 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 2323 | 2512 | 189 | 507 | 517 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 1282 | 2000 | 718 | 540 | 542 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 86.8 | 817 | 730 | 75 | 237 |

Preact app (TSX):

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 492 | - | - | 207 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 22.7 | 518 | 495 | 17 | 208 |
| rolldown: rolldown, then `tsc --noEmit` | 99.1 | 589 | 490 | 134 | 208 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 120 | 772 | 652 | 114 | 391 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 171 | 667 | 496 | 155 | 208 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 657 | 974 | 317 | 214 | 567 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 941 | 1086 | 145 | 311 | 318 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 601 | 1081 | 480 | 335 | 336 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 29.1 | 528 | 499 | 44 | 208 |

Monorepo-style TypeScript app:

| Tool and setup | Build only (ms) | Build + check (ms) | The check adds (ms) | Memory build only (MB) | Memory build + check (MB) |
|---|---|---|---|---|---|
| `tsc --noEmit` alone | - | 747 | - | - | 230 |
| Rathkit Pack: no type checker | not measured | not measured | - | - | - |
| esbuild: esbuild, then `tsc --noEmit` | 39.7 | 800 | 760 | 32 | 231 |
| rolldown: rolldown, then `tsc --noEmit` | 114 | 852 | 738 | 161 | 232 |
| rspack: ts-checker-rspack-plugin (`async: false`) in the rspack build | 255 | 1031 | 776 | 144 | 446 |
| Vite: `tsc --noEmit`, then `vite build` (the starter template runs `tsc -b && vite build`) | 188 | 924 | 736 | 182 | 231 |
| webpack: fork-ts-checker-webpack-plugin (`async: false`) in the webpack build | 2632 | 2844 | 212 | 442 | 839 |
| Rollup: @rollup/plugin-typescript with checking on (its default), reading the project tsconfig | 1396 | 1655 | 259 | 380 | 401 |
| Parcel 2: Parcel 2, then `tsc --noEmit` | 871 | 1631 | 760 | 491 | 490 |
| Parcel 3: Parcel 3, then `tsc --noEmit` | 48.9 | 792 | 743 | 65 | 231 |
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
