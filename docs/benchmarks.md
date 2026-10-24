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
  0.1 before the run). Node 26.12.0; rustc 1.99.1; rathkit-pack at commit `99e7057` (release build); esbuild 0.28.2,
  rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5.

Reproduce: `cargo build --release && cd bench && npm install && node generate.mjs && node run.mjs --tools rathkit,esbuild,rolldown,rspack,vite --markdown`.

## Time (median ms, lower is better)

Plain:

| | rathkit-pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 181 | 413 | 772 | 384 | 965 |
| three.js | 44 | 99 | 158 | 170 | 253 |
| lodash-es | 26 | 68 | 173 | 160 | 241 |
| React + react-dom/server | 14 | 51 | 121 | 114 | 193 |
| rxjs | 13 | 54 | 159 | 136 | 253 |
| 1,500-module app | 191 | 527 | 269 | 643 | 380 |
| 500-module TypeScript app | 67 | 90 | 174 | 377 | 266 |

Minified:

| | rathkit-pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 401 | 419 | 926 | 1931 | 1152 |
| three.js | 50 | 107 | 166 | 350 | 268 |
| lodash-es | 28 | 67 | 171 | 207 | 240 |
| React + react-dom/server | 22 | 52 | 123 | 203 | 197 |
| rxjs | 16 | 56 | 164 | 193 | 214 |
| 1,500-module app | 193 | 545 | 274 | 853 | 377 |
| 500-module TypeScript app | 75 | 92 | 173 | 574 | 261 |

Minified with a source map:

| | rathkit-pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 530 | 609 | 1058 | 2184 | 1222 |
| three.js | 72 | 123 | 185 | 458 | 283 |
| lodash-es | 40 | 71 | 187 | 218 | 248 |
| React + react-dom/server | 39 | 59 | 136 | 221 | 206 |
| rxjs | 23 | 63 | 178 | 207 | 229 |
| 1,500-module app | 215 | 549 | 291 | 897 | 393 |
| 500-module TypeScript app | 82 | 96 | 182 | 608 | 278 |

Geometric mean of each tool's time relative to rathkit-pack, over these seven workloads: plain, esbuild 2.6x, rolldown
4.5x, rspack 5.0x, Vite 6.7x; minified, esbuild 2.1x, rolldown 3.7x, rspack 7.1x, Vite 5.2x; minified with a map,
esbuild 1.7x, rolldown 3.0x, rspack 5.8x, Vite 4.1x. On the TypeScript compiler, minified, the gap to esbuild is small
(401 ms against 419 ms).

## Peak memory (MB, minified with a source map)

| | rathkit-pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 228 | 266 | 509 | 431 | 457 |
| three.js | 43 | 61 | 201 | 182 | 229 |
| lodash-es | 22 | 60 | 257 | 149 | 277 |
| React + react-dom/server | 20 | 48 | 189 | 132 | 218 |
| rxjs | 10 | 53 | 244 | 135 | 218 |
| 1,500-module app | 92 | 384 | 490 | 366 | 514 |
| 500-module TypeScript app | 30 | 57 | 198 | 186 | 216 |

## Output size (KB / gzip KB, minified)

| | rathkit-pack | esbuild | rolldown | rspack | Vite |
|---|---|---|---|---|---|
| TypeScript compiler | 3484 / 973 | 3514 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 |
| three.js | 524 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 |
| lodash-es | 76 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 114 / 31 |
| React + react-dom/server | 227 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 |
| rxjs | 25 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 33 / 8 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 (*) | 155 / 24 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 |

(*) rspack folds the whole generated app to a constant, so its size says nothing about that case. Different tools
pick different package entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Where others are smaller:** minified, rolldown is smaller than rathkit-pack on the TypeScript compiler (3472 against
3484 KB; gzip 970 against 973), three.js, and lodash-es; rspack is smaller on the TypeScript compiler (3405 / 965); both
are within a few percent. Plain (not minified), esbuild is smaller on three.js (1090 against 1220 KB), and rolldown and
Vite are smaller on React (488 and 470 against 580 KB).
