# Benchmarks

Measured, with the method below, on one machine. Other machines, other versions and other inputs give other numbers;
run the container recipe in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) to get your own. Nothing here is a claim about inputs that were not measured.

## Method

- **What is timed:** the whole command from start to exit, start-up included (`/usr/bin/time`), after one warm-up run;
  the median of 7 runs is shown. Peak memory is the largest resident size of the process.
- **What is compared:** the same entry, producing one ES module file for Node, with `process.env.NODE_ENV` defined as
  `"production"` in every tool, in three modes: plain, minified, and minified with a source map.
- **Do the outputs work:** each output that can run is run, and what it prints is compared with the unbundled program
  ("same" / "fails"). esbuild is given a banner that defines `require`, `__filename` and `__dirname`, so its ES module
  output can run the CommonJS TypeScript compiler and React. Where another bundler "fails" the same way (rolldown and Vite
  on the TypeScript compiler, Vite on React) or does not finish (Parcel 2 on the 1,500-module app: an internal assertion
  error in all three modes), it is not counted against the speed. Parcel 2's TypeScript compiler output also fails to run.
- **Parcel:** Parcel 2 and Parcel 3 are built from the `targets` in pack-bench's `package.json` (Node, ES module, node_modules
  included). Parcel 2 is run without its disk cache. Parcel 3 is run as its native executable, so its time has no Node
  start-up in it. Without `--optimize` Parcel does not minify and does not scope-hoist, so its plain build is not the same
  kind of build as the others.
- **Other tools** are used with the options listed in pack-bench's `run.mjs` (for rspack, a config with `builtin:swc-loader` for
  TypeScript; for Vite, library mode), not tuned beyond that. A better configuration of any of them may do better.
- **Workloads:** the TypeScript 5.9 compiler (one 9 MB CommonJS file), three.js (a few classes), lodash-es (`export *`),
  React with react-dom/server, an rxjs pipeline, and two generated apps (1,500 JavaScript modules and 500 TypeScript
  modules, pack-bench's `generate.mjs`). three.js and lodash are timed but not run.
<!-- pack-bench:begin -->
- **Machine:** Intel Core i7-11850H (16 threads), 31 GB, Linux 6.6 under WSL2. The machine was not idle: other programs
  were running and the load average was 2.2 before the run, so every figure is slower than on a quiet machine and the
  small gaps are noisy. Node 26.12.0; rustc 1.99.1; Rathkit Pack at commit `327a676` (release build); esbuild 0.28.2,
  rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, Parcel 2.16.4, Parcel 3.0.0-alpha.2 (an alpha).

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench), build Pack at the commit named above with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --tools rathkit,esbuild,rolldown,rspack,vite,parcel2,parcel3 --markdown`. These figures were made on the host, before the container recipe existed; they are replaced by a container run.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 213 | 448 | 754 | 395 | 975 | 2460 | 627 |
| three.js | 48 | 109 | 170 | 178 | 264 | 820 | 112 |
| lodash-es | 28 | 77 | 170 | 168 | 244 | 1265 | 9 |
| React + react-dom/server | 16 | 56 | 129 | 121 | 205 | 693 | 45 |
| rxjs | 18 | 59 | 156 | 137 | 209 | 832 | 44 |
| 1,500-module app | 197 | 562 | 282 | 697 | 376 | failed | 236 |
| 500-module TypeScript app | 78 | 96 | 178 | 392 | 259 | 1694 | 96 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 412 | 436 | 927 | 1903 | 1175 | 3973 | 1551 |
| three.js | 56 | 117 | 171 | 358 | 281 | 1009 | 154 |
| lodash-es | 31 | 76 | 176 | 216 | 248 | 1375 | 11 |
| React + react-dom/server | 24 | 58 | 130 | 203 | 208 | 892 | 81 |
| rxjs | 17 | 60 | 163 | 193 | 222 | 857 | 47 |
| 1,500-module app | 212 | 576 | 272 | 906 | 374 | failed | 266 |
| 500-module TypeScript app | 75 | 99 | 177 | 551 | 273 | 1684 | 101 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 544 | 616 | 1041 | 2204 | 1235 | 5322 | 1665 |
| three.js | 81 | 140 | 206 | 422 | 310 | 1293 | 165 |
| lodash-es | 43 | 82 | 192 | 230 | 259 | 1440 | 12 |
| React + react-dom/server | 34 | 61 | 141 | 222 | 217 | 996 | 88 |
| rxjs | 24 | 71 | 181 | 206 | 222 | 890 | 56 |
| 1,500-module app | 228 | 583 | 295 | 902 | 409 | failed | 244 |
| 500-module TypeScript app | 87 | 103 | 185 | 602 | 277 | 1989 | 92 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 2.5x, rolldown 4.1x, rspack 4.6x, Vite 5.8x, Parcel 2 27.2x, Parcel 3 1.6x;
minified, esbuild 2.1x, rolldown 3.5x, rspack 6.7x, Vite 5.1x, Parcel 2 26.3x, Parcel 3 1.8x; minified with a map, esbuild 1.8x, rolldown 2.9x, rspack 5.6x, Vite 4.0x, Parcel 2 22.4x, Parcel 3 1.4x. On the TypeScript compiler, minified, the gap to
esbuild is small (412 ms against 436 ms).

## Peak memory (MB, minified with a source map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 229 | 265 | 519 | 421 | 479 | 1396 | 380 |
| three.js | 43 | 61 | 198 | 185 | 229 | 563 | 143 |
| lodash-es | 22 | 61 | 251 | 150 | 279 | 550 | 30 |
| React + react-dom/server | 21 | 48 | 205 | 132 | 220 | 507 | 183 |
| rxjs | 11 | 54 | 241 | 135 | 220 | 431 | 108 |
| 1,500-module app | 94 | 383 | 480 | 366 | 520 | failed | 282 |
| 500-module TypeScript app | 31 | 58 | 197 | 183 | 212 | 584 | 107 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 3485 / 973 | 3515 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 | 3406 / 965 | 3425 / 969 |
| three.js | 524 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 | 519 / 128 | 617 / 152 |
| lodash-es | 76 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 | 85 / 29 | 3 / 1 |
| React + react-dom/server | 228 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 | 583 / 175 | 585 / 176 |
| rxjs | 25 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 | 22 / 7 | 175 / 39 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 | failed | 406 / 67 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 | 173 / 30 | 271 / 29 |
<!-- pack-bench:end -->

(*) rspack folds the whole generated app to a constant, so its size says nothing about that case. Parcel 3's 3 KB for
lodash-es is also not a comparable result: nothing the entry re-exports was kept. Different tools pick different package
entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Where others are smaller:** minified, rolldown is smaller than Rathkit Pack on the TypeScript compiler (3472 against
3485 KB; gzip 970 against 973), three.js, and lodash-es; rspack is smaller on the TypeScript compiler (3405 / 965); both
are within a few percent. Plain (not minified), esbuild is smaller on three.js (1091 against 1220 KB), rolldown and Vite
are smaller on React (488 and 471 against 580 KB), and rolldown and Vite are smaller on the TypeScript compiler (7774
and 7683 against 8967 KB).

**Parcel:** minified, Parcel 2 is smaller than Rathkit Pack on rxjs (22 against 25 KB) and on the TypeScript compiler
(3406 against 3485 KB; gzip 965 against 973), and Parcel 3 is within 2% on the TypeScript compiler (3425 KB). Parcel 3
is larger on three.js, React, rxjs and both generated apps (its plain build of the 1,500-module app is 6990 KB, against 126
KB for Rathkit Pack). Parcel 3 is an alpha. Against Parcel 2 it was between 2.6x and 21.5x faster on the inputs both built
and used less memory, apart from the lodash-es case where it kept nothing; this was not close to the 100x its
announcement gives as the best case, which was measured on other, larger inputs.
