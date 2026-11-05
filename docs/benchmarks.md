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
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.54.1-microsoft-standard-WSL2; WSL2, Docker Desktop 29.9.2, laptop on AC. The machine was idle: the load average was 0.27, 0.8, 0.99 before the run and 2.5, 1.64, 1.27 after it. Node 26.13.0; Rathkit Pack 0.1.0-alpha.1 at commit `4342729` (release build, sha256 `54de6970dedc`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha). Run on 2026-11-05 in the pack-bench container, bench commit `77baa2f`, image `0a0d32b93eca`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `77baa2f`, build Pack at commit `4342729` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261105T205648Z.json --tools rathkit,esbuild,rolldown,rspack,vite,parcel2,parcel3 --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 171 | 352 | 663 | 330 | 858 | 2203 | 570 |
| three.js | 38 | 60 | 133 | 141 | 225 | 704 | 88 |
| lodash-es | 22 | 32 | 111 | 121 | 180 | 1047 | 7 |
| React + react-dom/server | 11 | 17 | 100 | 94 | 171 | 593 | 33 |
| rxjs | 12 | 20 | 101 | 103 | 158 | 679 | 21 |
| 1,500-module app | 135 | 481 | 212 | 569 | 305 | failed | 190 |
| 500-module TypeScript app | 53 | 51 | 137 | 324 | 218 | 1178 | 69 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 369 | 368 | 827 | 1718 | 1012 | 3644 | 1379 |
| three.js | 45 | 67 | 147 | 294 | 233 | 835 | 128 |
| lodash-es | 26 | 31 | 114 | 159 | 181 | 1096 | 7 |
| React + react-dom/server | 19 | 18 | 109 | 167 | 172 | 735 | 60 |
| rxjs | 14 | 21 | 108 | 150 | 157 | 685 | 25 |
| 1,500-module app | 142 | 485 | 210 | 774 | 300 | failed | 175 |
| 500-module TypeScript app | 57 | 54 | 140 | 465 | 219 | 1202 | 73 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 479 | 522 | 917 | 1953 | 1107 | 4759 | 1485 |
| three.js | 62 | 84 | 158 | 338 | 243 | 1037 | 143 |
| lodash-es | 34 | 36 | 121 | 170 | 184 | 1150 | 7 |
| React + react-dom/server | 26 | 22 | 110 | 181 | 177 | 839 | 72 |
| rxjs | 18 | 26 | 117 | 159 | 161 | 721 | 29 |
| 1,500-module app | 160 | 494 | 223 | 789 | 310 | failed | 182 |
| 500-module TypeScript app | 64 | 58 | 145 | 512 | 223 | 1378 | 74 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 1.7x, rolldown 4.1x, rspack 4.9x, Vite 6.4x, Parcel 2 30.2x, Parcel 3 1.6x;
minified, esbuild 1.4x, rolldown 3.4x, rspack 7.0x, Vite 5.1x, Parcel 2 26.2x, Parcel 3 1.6x;
minified with a map, esbuild 1.3x, rolldown 2.8x, rspack 6.0x, Vite 4.1x, Parcel 2 23.1x, Parcel 3 1.4x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | 221 | 265 | 497 | 416 | 466 | 1377 | 368 |
| three.js | 39 | 59 | 205 | 185 | 242 | 564 | 143 |
| lodash-es | 21 | 56 | 233 | 145 | 261 | 535 | 27 |
| React + react-dom/server | 19 | 29 | 202 | 131 | 211 | 506 | 170 |
| rxjs | 9 | 49 | 224 | 130 | 206 | 426 | 88 |
| 1,500-module app | 92 | 380 | 466 | 353 | 493 | failed | 240 |
| 500-module TypeScript app | 27 | 55 | 188 | 179 | 205 | 576 | 83 |

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

## Does the output run and print the same? (plain)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|
| TypeScript compiler | same | same | fails | same | fails | fails | same |
| three.js | - | - | - | - | - | - | - |
| lodash-es | - | - | - | - | - | - | - |
| React + react-dom/server | same | same | same | same | fails | same | same |
| rxjs | same | same | same | same | same | same | same |
| 1,500-module app | same | same | same | same | same | failed | same |
| 500-module TypeScript app | same | same | same | same | same | same | same |
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
KB for Rathkit Pack). Parcel 3 is an alpha. Against Parcel 2 it was between 2.6x and 32.8x faster on the inputs both built
and used less memory, apart from the lodash-es case where it kept nothing; this was not close to the 100x its
announcement gives as the best case, which was measured on other, larger inputs.
