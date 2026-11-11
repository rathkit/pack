# Benchmarks

Measured, with the method below, on one machine. Other machines, other versions and other inputs give other numbers;
run the container recipe in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) to get your own. Nothing here is a claim about inputs that were not measured.

## Method

- **What is timed:** the whole command from start to exit, start-up included (`/usr/bin/time`), after one warm-up run;
  the median of 7 runs is shown. Peak memory is the largest resident size of the process.
- **What is compared:** the same entry, producing one ES module file for Node, with `process.env.NODE_ENV` defined as
  `"production"` in every tool, in three modes: plain, minified, and minified with a source map.
- **Do the outputs work:** each output that can run is run, and what it prints is compared with the unbundled program
  ("same" / "fails"). esbuild is run with `--platform=node` and is given a banner that defines `require`, `__filename` and `__dirname`, so its ES module
  output can run the CommonJS TypeScript compiler and React. Where another bundler "fails" the same way (rolldown, Vite and Rollup
  on the TypeScript compiler, Vite on React) or does not finish (Parcel 2 on the 1,500-module app: an internal assertion
  error in all three modes), it is not counted against the speed. Parcel 2's TypeScript compiler output also fails to run.
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
- **Workloads:** the TypeScript 5.9 compiler (one 9 MB CommonJS file), three.js (a few classes), lodash-es (`export *`),
  React with react-dom/server, an rxjs pipeline, and two generated apps (1,500 JavaScript modules and 500 TypeScript
  modules, pack-bench's `generate.mjs`). three.js and lodash are timed but not run.
<!-- pack-bench:begin -->
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.58.1-microsoft-standard-WSL2; WSL2, Docker Desktop, laptop on AC. The machine was idle: the load average was 0.99, 0.27, 0.14 before the run and 2.13, 2.04, 1.68 after it. Node 26.13.0; Rathkit Pack 0.1.0-alpha.2 at commit `e85c76b` (release build, sha256 `88ab2d7c1102`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha). Run on 2026-11-11 in the pack-bench container, bench commit `ebd9e98`, image `824c1d882604`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `ebd9e98`, build Pack at commit `e85c76b` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261111T142341Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --runs 7 --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 182 | 384 | 709 | 342 | 861 | 1248 | 7906 | 2168 | 559 |
| three.js | 38 | 58 | 130 | 143 | 228 | 579 | 1812 | 687 | 81 |
| lodash-es | 20 | 32 | 109 | 116 | 180 | 733 | 3020 | 1039 | 7 |
| React + react-dom/server | 11 | 17 | 104 | 93 | 166 | 381 | 1593 | 598 | 34 |
| rxjs | 10 | 20 | 102 | 101 | 157 | 542 | 2298 | 671 | 22 |
| 1,500-module app | 95 | 479 | 209 | 569 | 306 | 1836 | 4924 | failed | 190 |
| 500-module TypeScript app | 26 | 53 | 138 | 323 | 215 | 909 | 1630 | 1176 | 70 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 240 | 362 | 815 | 1687 | 1019 | 18234 | 26914 | 3568 | 1388 |
| three.js | 44 | 67 | 143 | 294 | 251 | 2452 | 2975 | 837 | 129 |
| lodash-es | 23 | 32 | 114 | 160 | 185 | 1437 | 3480 | 1089 | 8 |
| React + react-dom/server | 16 | 18 | 107 | 165 | 177 | 1416 | 2321 | 756 | 62 |
| rxjs | 12 | 20 | 112 | 151 | 162 | 1414 | 2534 | 685 | 27 |
| 1,500-module app | 98 | 477 | 210 | 771 | 301 | 3963 | 4951 | failed | 175 |
| 500-module TypeScript app | 30 | 53 | 140 | 468 | 221 | 3774 | 2369 | 1238 | 73 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 304 | 522 | 903 | 1949 | 1106 | 18784 | 27864 | 4666 | 1490 |
| three.js | 55 | 85 | 157 | 334 | 248 | 2653 | 3067 | 1035 | 145 |
| lodash-es | 31 | 36 | 118 | 172 | 189 | 1503 | 3508 | 1150 | 8 |
| React + react-dom/server | 21 | 22 | 115 | 185 | 184 | 1491 | 2397 | 857 | 67 |
| rxjs | 15 | 27 | 120 | 169 | 163 | 1471 | 2542 | 698 | 29 |
| 1,500-module app | 116 | 489 | 221 | 790 | 314 | 4111 | 5002 | failed | 178 |
| 500-module TypeScript app | 38 | 56 | 144 | 513 | 227 | 3800 | 2446 | 1386 | 75 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 2.1x, rolldown 5.0x, rspack 5.9x, Vite 7.7x, webpack 24.2x, Rollup 85.6x, Parcel 2 35.0x, Parcel 3 1.9x;
minified, esbuild 1.8x, rolldown 4.4x, rspack 9.1x, Vite 6.7x, webpack 75.1x, Rollup 104.3x, Parcel 2 33.6x, Parcel 3 2.1x;
minified with a map, esbuild 1.7x, rolldown 3.7x, rspack 8.0x, Vite 5.5x, webpack 62.3x, Rollup 84.9x, Parcel 2 30.2x, Parcel 3 1.8x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 201 | 265 | 505 | 430 | 462 | 1773 | 3141 | 1392 | 375 |
| three.js | 38 | 59 | 203 | 186 | 232 | 491 | 640 | 565 | 143 |
| lodash-es | 21 | 56 | 233 | 144 | 267 | 348 | 490 | 536 | 27 |
| React + react-dom/server | 17 | 29 | 187 | 131 | 211 | 326 | 518 | 506 | 186 |
| rxjs | 9 | 49 | 224 | 130 | 204 | 342 | 401 | 429 | 92 |
| 1,500-module app | 91 | 381 | 466 | 353 | 499 | 1039 | 794 | failed | 240 |
| 500-module TypeScript app | 26 | 55 | 192 | 178 | 207 | 532 | 515 | 572 | 83 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 3477 / 975 | 3515 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 | 3467 / 959 | 3458 / 959 | 3406 / 965 | 3425 / 969 |
| three.js | 515 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 | 524 / 126 | 506 / 123 | 519 / 128 | 617 / 152 |
| lodash-es | 76 / 27 | 81 / 30 | 73 / 26 | 78 / 29 | 132 / 32 | 72 / 25 | 71 / 24 | 85 / 29 | 3 / 1 |
| React + react-dom/server | 228 / 68 | 229 / 69 | 228 / 68 | 225 / 68 | 275 / 71 | 226 / 68 | 227 / 67 | 583 / 175 | 585 / 176 |
| rxjs | 25 / 7 | 157 / 34 | 151 / 27 | 140 / 28 | 35 / 8 | 143 / 28 | 22 / 7 | 22 / 7 | 175 / 39 |
| 1,500-module app | 80 / 20 | 92 / 25 | 80 / 20 | 1 / 0 | 155 / 24 | 80 / 17 | 3 / 1 | failed | 406 / 67 |
| 500-module TypeScript app | 172 / 20 | 187 / 23 | 198 / 20 | 512 / 41 | 263 / 22 | 541 / 32 | 185 / 21 | 173 / 30 | 271 / 29 |

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
<!-- pack-bench:end -->

(*) rspack and Rollup fold the whole generated app to a constant (1 and 3 KB), so their sizes say nothing about that case. Parcel 3's 3 KB for
lodash-es is also not a comparable result: nothing the entry re-exports was kept. Different tools pick different package
entry points (rxjs has an ES and a CommonJS build), which moves sizes.

**Against esbuild** the output of Rathkit Pack is smaller in all 21 cases (gzip too, except for plain React).

**Where others are smaller:** minified, rolldown, rspack, webpack, Rollup, Parcel 2 and Parcel 3 are each smaller than
Rathkit Pack on the TypeScript compiler (3405 to 3472 KB against 3477 KB, within about 2%; gzip 959 to 970 against 975);
rolldown and Rollup are smaller on three.js (513 and 506 against 515 KB); rolldown, webpack and Rollup on lodash-es (73,
72 and 71 against 76 KB); rolldown, rspack, webpack and Rollup on React by up to 3 KB; and Rollup and Parcel 2 on rxjs
(22 against 25 KB). Plain (not minified), rolldown and Vite are smaller on the TypeScript compiler (7774 and 7683 against
8216 KB) and on React (488 and 471 against 578 KB), and Vite and Rollup are smaller on rxjs (54 and 56 against 60 KB).

**Parcel:** minified, Parcel 2 is smaller than Rathkit Pack on rxjs (22 against 25 KB) and on the TypeScript compiler
(3406 against 3477 KB; gzip 965 against 975), and Parcel 3 is smaller on the TypeScript compiler (3425 KB). Parcel 3
is larger on three.js, React, rxjs and both generated apps (its plain build of the 1,500-module app is 6990 KB, against 126
KB for Rathkit Pack). Parcel 3 is an alpha. Against Parcel 2 it was between 2.6x and 30.7x faster on the inputs both built
and used less memory, apart from the lodash-es case where it kept nothing; this was not close to the 100x its
announcement gives as the best case, which was measured on other, larger inputs.
