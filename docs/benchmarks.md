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
- **Machine:** 11th Gen Intel(R) Core(TM) i7-11850H @ 2.50GHz (8 logical CPUs available to the container, cpuset 2-9), memory limit 12 GB, Linux 6.18.54.1-microsoft-standard-WSL2; WSL2, Docker Desktop, laptop on AC. The machine was idle: the load average was 0.13, 0.1, 0.09 before the run and 2.28, 2, 1.67 after it. Node 26.13.0; Rathkit Pack 0.1.0-alpha.1 at commit `317549a` (release build, sha256 `22ee8cdffaa4`); esbuild 0.28.2, rolldown 1.2.15, @rspack/core 2.2.9, Vite 8.3.5, webpack 5.112.2 (webpack-cli 7.3.1, swc-loader 0.2.7, @swc/core 1.16.22), Rollup 4.67.5 (node-resolve 16.0.3, commonjs 29.0.3, replace 6.0.3, typescript 12.3.1, terser 1.0.2), Parcel 2 2.16.4, Parcel 3 3.0.0-alpha.2 (an alpha). Run on 2026-11-08 in the pack-bench container, bench commit `d4d8537`, image `c277a44dd69e`; tools started through their own executables; 7 timed runs, outputs on a RAM disk.

Reproduce: in [rathkit/pack-bench](https://github.com/rathkit/pack-bench) at commit `d4d8537`, build Pack at commit `317549a` with `container/build-pack.sh`, then run `container/run.sh --pack-bin <that binary> -- --work /work --results /results/20261108T012538Z.json --tools rathkit,esbuild,rolldown,rspack,vite,webpack,rollup,parcel2,parcel3 --runs 7 --markdown`.

## Time (median ms, lower is better)

Plain:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 186 | 360 | 682 | 327 | 855 | 1241 | 7849 | 2187 | 557 |
| three.js | 39 | 59 | 136 | 140 | 236 | 579 | 1829 | 709 | 80 |
| lodash-es | 23 | 34 | 109 | 121 | 180 | 730 | 3001 | 1047 | 7 |
| React + react-dom/server | 11 | 17 | 103 | 94 | 170 | 379 | 1598 | 591 | 35 |
| rxjs | 12 | 20 | 104 | 99 | 161 | 545 | 2313 | 675 | 22 |
| 1,500-module app | 136 | 477 | 211 | 567 | 300 | 1820 | 4910 | failed | 190 |
| 500-module TypeScript app | 53 | 52 | 138 | 320 | 217 | 913 | 1641 | 1181 | 70 |

Minified:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 364 | 357 | 820 | 1696 | 1020 | 18365 | 26952 | 3562 | 1385 |
| three.js | 45 | 69 | 144 | 294 | 236 | 2470 | 2955 | 846 | 127 |
| lodash-es | 26 | 32 | 114 | 157 | 183 | 1426 | 3491 | 1091 | 6 |
| React + react-dom/server | 19 | 18 | 107 | 165 | 171 | 1409 | 2316 | 748 | 61 |
| rxjs | 15 | 20 | 108 | 153 | 161 | 1417 | 2540 | 692 | 27 |
| 1,500-module app | 144 | 487 | 216 | 770 | 314 | 3967 | 4990 | failed | 174 |
| 500-module TypeScript app | 57 | 55 | 138 | 464 | 221 | 3805 | 2375 | 1224 | 73 |

Minified with a source map:

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 475 | 524 | 908 | 1964 | 1102 | 19045 | 28069 | 4679 | 1511 |
| three.js | 62 | 84 | 166 | 340 | 243 | 2648 | 3073 | 1044 | 142 |
| lodash-es | 34 | 37 | 118 | 171 | 189 | 1497 | 3529 | 1141 | 7 |
| React + react-dom/server | 26 | 22 | 114 | 182 | 181 | 1502 | 2384 | 866 | 71 |
| rxjs | 19 | 26 | 118 | 161 | 163 | 1480 | 2536 | 695 | 29 |
| 1,500-module app | 159 | 492 | 226 | 789 | 313 | 4172 | 5014 | failed | 183 |
| 500-module TypeScript app | 63 | 57 | 142 | 512 | 219 | 3799 | 2448 | 1396 | 76 |

Geometric mean of each tool's time relative to Rathkit Pack, over the workloads each tool built: plain, esbuild 1.7x, rolldown 4.1x, rspack 4.8x, Vite 6.3x, webpack 19.7x, Rollup 69.9x, Parcel 2 29.5x, Parcel 3 1.5x;
minified, esbuild 1.3x, rolldown 3.3x, rspack 6.9x, Vite 5.1x, webpack 57.1x, Rollup 79.2x, Parcel 2 25.9x, Parcel 3 1.6x;
minified with a map, esbuild 1.3x, rolldown 2.8x, rspack 5.9x, Vite 4.1x, webpack 47.2x, Rollup 64.1x, Parcel 2 23.0x, Parcel 3 1.4x.

## Peak memory (MB, minified with a map)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 221 | 265 | 494 | 417 | 468 | 1773 | 3117 | 1390 | 374 |
| three.js | 40 | 60 | 203 | 188 | 222 | 491 | 638 | 566 | 141 |
| lodash-es | 21 | 57 | 235 | 144 | 266 | 349 | 491 | 538 | 27 |
| React + react-dom/server | 19 | 29 | 193 | 131 | 213 | 325 | 518 | 504 | 182 |
| rxjs | 9 | 49 | 222 | 131 | 208 | 344 | 402 | 429 | 92 |
| 1,500-module app | 92 | 380 | 464 | 353 | 499 | 1051 | 794 | failed | 248 |
| 500-module TypeScript app | 27 | 55 | 190 | 178 | 203 | 533 | 515 | 570 | 83 |

## Output size (KB / gzip KB, minified)

| | Rathkit Pack | esbuild | rolldown | rspack | Vite | webpack | Rollup | Parcel 2 | Parcel 3 |
|---|---|---|---|---|---|---|---|---|---|
| TypeScript compiler | 3485 / 973 | 3515 / 1012 | 3472 / 970 | 3405 / 965 | 4385 / 1052 | 3467 / 959 | 3458 / 959 | 3406 / 965 | 3425 / 969 |
| three.js | 524 / 128 | 517 / 130 | 513 / 126 | 519 / 128 | 601 / 134 | 524 / 126 | 506 / 123 | 519 / 128 | 617 / 152 |
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
