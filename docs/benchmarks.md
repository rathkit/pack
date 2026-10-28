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
  0.3 before the run). Node 26.12.0; rustc 1.99.1; Rathkit Pack at commit `85e4c85` (release build); esbuild 0.28.2,
  rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5.

Reproduce: `cargo build --release && cd bench && pnpm install && node generate.mjs && node run.mjs --tools rathkit,esbuild,rolldown,rspack,vite --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 174 | 391 | 698 | 367 | 908 |
| three.js | 41 | 95 | 149 | 158 | 244 |
| lodash-es | 25 | 68 | 162 | 158 | 227 |
| React + react-dom/server | 14 | 50 | 117 | 109 | 183 |
| rxjs | 14 | 54 | 147 | 133 | 201 |
| 1,500-module app | 183 | 495 | 253 | 621 | 348 |
| 500-module TypeScript app | 67 | 86 | 161 | 360 | 239 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 363 | 397 | 878 | 1738 | 1053 |
| three.js | 48 | 100 | 156 | 322 | 253 |
| lodash-es | 28 | 67 | 165 | 203 | 236 |
| React + react-dom/server | 21 | 49 | 122 | 197 | 193 |
| rxjs | 16 | 54 | 158 | 185 | 209 |
| 1,500-module app | 186 | 497 | 255 | 833 | 349 |
| 500-module TypeScript app | 70 | 88 | 169 | 511 | 254 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 487 | 561 | 958 | 2107 | 1140 |
| three.js | 67 | 117 | 174 | 368 | 267 |
| lodash-es | 38 | 73 | 177 | 215 | 247 |
| React + react-dom/server | 31 | 54 | 130 | 210 | 197 |
| rxjs | 21 | 62 | 171 | 193 | 213 |
| 1,500-module app | 205 | 518 | 272 | 850 | 366 |
| 500-module TypeScript app | 79 | 90 | 170 | 562 | 258 |

Geometric mean of each tool's time relative to Rathkit Pack, over these seven workloads: plain, esbuild 2.6x, rolldown
4.4x, rspack 5.0x, Vite 6.3x; minified, esbuild 2.0x, rolldown 3.7x, rspack 7.0x, Vite 5.2x; minified with a map,
esbuild 1.8x, rolldown 3.0x, rspack 5.9x, Vite 4.2x. On the TypeScript compiler, minified, the gap to esbuild is small
(363 ms against 397 ms).

## Peak memory (MB, minified with a source map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 229 | 265 | 521 | 442 | 451 |
| three.js | 43 | 60 | 201 | 182 | 236 |
| lodash-es | 22 | 61 | 254 | 149 | 287 |
| React + react-dom/server | 21 | 48 | 195 | 133 | 212 |
| rxjs | 11 | 54 | 244 | 133 | 217 |
| 1,500-module app | 94 | 384 | 488 | 365 | 514 |
| 500-module TypeScript app | 31 | 57 | 199 | 184 | 210 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 3485 / 973 | 3514 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 |
| three.js | 524 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 |
| lodash-es | 76 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 |
| React + react-dom/server | 228 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 |
| rxjs | 25 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 |

(*) rspack folds the whole generated app to a constant, so its size says nothing about that case. Different tools
pick different package entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Where others are smaller:** minified, rolldown is smaller than Rathkit Pack on the TypeScript compiler (3472 against
3485 KB; gzip 970 against 973), three.js, and lodash-es; rspack is smaller on the TypeScript compiler (3405 / 965); both
are within a few percent. Plain (not minified), esbuild is smaller on three.js (1090 against 1220 KB), rolldown and Vite
are smaller on React (488 and 471 against 580 KB), and rolldown and Vite are smaller on the TypeScript compiler (7774
and 7683 against 8967 KB).
