# Benchmarks

Measured, with the method below, on one machine. Other machines, other versions and other inputs give other numbers;
run `bench/run.mjs` to get your own. Nothing here is a claim about inputs that were not measured.

## Method

- **What is timed:** the whole command from start to exit, start-up included (`/usr/bin/time`), after one warm-up run;
  the median of 7 runs is shown. Peak memory is the largest resident size of the process.
- **What is compared:** the same entry, producing one ES module file for Node, with `process.env.NODE_ENV` defined as
  `"production"` in every tool, in three modes: plain, minified, and minified with a source map.
- **Do the outputs work:** each output that can run is run, and what it prints is compared with the unbundled program
  ("same" / "fails"). Where a bundler "fails" on the TypeScript compiler it is because that file is CommonJS and
  uses `require`, which an ES module output cannot do without a banner; this is not counted against the speed.
- **Other tools** are used with the options listed in `bench/run.mjs` (for rspack, a config with `builtin:swc-loader` for
  TypeScript; for Vite, library mode), not tuned beyond that. A better configuration of any of them may do better.
- **Workloads:** the TypeScript 5.9 compiler (one 9 MB CommonJS file), three.js (a few classes), lodash-es (`export *`),
  React with react-dom/server, an rxjs pipeline, and two generated apps (1,500 JavaScript modules and 500 TypeScript
  modules, `bench/generate.mjs`). three.js and lodash are timed but not run.
- **Machine:** Intel Core i7-11850H (16 threads), 31 GB, Linux 6.6 under WSL2, nothing else running (load average
  1.1 before the run). Node 26.12.0; rustc 1.99.1; Rathkit Pack at commit `a2105a3` (release build); esbuild 0.28.2,
  rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5.

Reproduce: `cargo build --release && cd bench && pnpm install && node generate.mjs && node run.mjs --tools rathkit,esbuild,rolldown,rspack,vite --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 181 | 407 | 742 | 380 | 983 |
| three.js | 46 | 104 | 161 | 170 | 273 |
| lodash-es | 27 | 76 | 187 | 202 | 268 |
| React + react-dom/server | 13 | 54 | 124 | 118 | 199 |
| rxjs | 14 | 60 | 161 | 139 | 211 |
| 1,500-module app | 191 | 549 | 270 | 651 | 364 |
| 500-module TypeScript app | 67 | 94 | 172 | 373 | 266 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 397 | 424 | 953 | 2049 | 1163 |
| three.js | 49 | 119 | 172 | 359 | 282 |
| lodash-es | 34 | 74 | 179 | 216 | 250 |
| React + react-dom/server | 24 | 56 | 136 | 205 | 203 |
| rxjs | 17 | 61 | 168 | 195 | 210 |
| 1,500-module app | 193 | 547 | 263 | 872 | 364 |
| 500-module TypeScript app | 76 | 94 | 174 | 545 | 261 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 539 | 638 | 1062 | 2286 | 1312 |
| three.js | 72 | 128 | 199 | 411 | 294 |
| lodash-es | 41 | 84 | 206 | 226 | 268 |
| React + react-dom/server | 32 | 60 | 137 | 222 | 208 |
| rxjs | 23 | 68 | 179 | 205 | 220 |
| 1,500-module app | 215 | 603 | 289 | 918 | 384 |
| 500-module TypeScript app | 84 | 98 | 181 | 599 | 270 |

Geometric mean of each tool's time relative to Rathkit Pack, over these seven workloads: plain, esbuild 2.7x, rolldown
4.5x, rspack 5.2x, Vite 6.6x; minified, esbuild 2.1x, rolldown 3.6x, rspack 6.9x, Vite 5.1x; minified with a map,
esbuild 1.9x, rolldown 3.1x, rspack 5.9x, Vite 4.2x. On the TypeScript compiler, minified, the gap to esbuild is small
(397 ms against 424 ms).

## Peak memory (MB, minified with a source map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 230 | 266 | 503 | 433 | 455 |
| three.js | 44 | 61 | 195 | 183 | 236 |
| lodash-es | 22 | 61 | 251 | 150 | 280 |
| React + react-dom/server | 20 | 48 | 201 | 133 | 217 |
| rxjs | 10 | 54 | 240 | 135 | 217 |
| 1,500-module app | 93 | 383 | 490 | 367 | 512 |
| 500-module TypeScript app | 30 | 58 | 196 | 184 | 219 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 3484 / 973 | 3514 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 |
| three.js | 524 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 |
| lodash-es | 76 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 |
| React + react-dom/server | 227 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 |
| rxjs | 25 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 |

(*) rspack folds the whole generated app to a constant, so its size says nothing about that case. Different tools
pick different package entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Where others are smaller:** minified, rolldown is smaller than Rathkit Pack on the TypeScript compiler (3472 against
3484 KB; gzip 970 against 973), three.js, and lodash-es; rspack is smaller on the TypeScript compiler (3405 / 965); both
are within a few percent. Plain (not minified), esbuild is smaller on three.js (1090 against 1220 KB), rolldown and Vite
are smaller on React (488 and 471 against 580 KB), and rolldown and Vite are smaller on the TypeScript compiler (7774
and 7683 against 8967 KB).
