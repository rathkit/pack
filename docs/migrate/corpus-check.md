# rkp migrate on the corpus projects

Roadmap 4.6 ("works on the corpus projects"). Method: the 63 projects of `corpus/projects.json` at their pinned commits (clones made by tools/run-corpus.mjs, dependencies installed with pnpm --ignore-scripts); on each, `rkp migrate DIR` was run; the printed `build:rkp` command was then run in a copy of the project (node_modules hard-linked, build output and `out` left out), and `rkp migrate DIR --write` was run in a copy to check that no script or other field of package.json that existed before was changed. The corpus runner's own command for the project (`build` in projects.json) is shown for comparison. Built: ok means rkp exited with 0 and wrote its output (the page and the files were not loaded in a browser in this first pass; `node tools/run-corpus.mjs --migrate` now does that for the printed commands, see below).

Summary: 63 projects; 34 have one of the tools; 30 print a command that builds (28 at the first pass, zustand and valtio since #121); none of the 34 --write runs changed an existing script or field.

Bugs this check found and fixed (tests in tests/migrate_command.rs): a vite.config that only configures Vitest or vite as a dependency of a library with no page printed a Vite command; Rollup configs with an input that is not a plain string printed the usage text as the command; a Rollup config that builds its output file in code printed a command that writes nothing; Vite base read from a variable was dropped without a word; rspack was not detected.

Known differences from the corpus runner's command (not bugs of migrate; the guide says so): migrate adds --minify for Vite and, since #124, carries --public-path when base is a constant or a variable that the project sets (vite-react-ts-less takes it from the constant VITE_BASE_PATH = '/' of config/constant.ts, so its command now has --public-path=/); the library projects are built by the runner with --format esm --outfile out/NAME.mjs, which no config of theirs states.

| Project | Detected | rkp migrate command | Built | --write |
|---|---|---|---|---|
| vue3-realworld | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| zustand | esbuild+rollup | `rkp bundle src/index.ts --outdir dist --splitting`, `... --outfile dist/index.js --format=cjs`, `... --outfile dist/esm/index.mjs` (the config is a function; it is run since #121) | ok (all three, run here after #121) | unchanged |
| dayjs | rollup | `rkp bundle src/index.js --outfile dayjs.min.js --format=iife --global-name=dayjs --minify` (the config is a function; the commands come from the plain `input` and `fileName` of the calls in build/index.js, and the calls whose names are made in a loop are left out) | ok (run here; the global is `dayjs.default`, the note says so) | unchanged |
| marked | esbuild | `rkp bundle src/marked.ts --outfile lib/marked.esm.js --minify --sourcemap` (entry point and plain strings of the `build()` calls in esbuild.config.js; the umd build has no command and a note says why) | ok (run here) | unchanged |
| conduit-react | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vue-naive-admin | vite | `rkp bundle index.html --config vite.config.js --outdir dist --minify` | ok | unchanged |
| vite-template-react-ts | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-template-vue-ts | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-template-svelte-ts | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-template-preact-ts | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-template-solid-ts | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-template-lit-ts | vite | `rkp bundle index.html --outdir dist --minify` | ok | unchanged |
| rspack-code-splitting | rspack | `rkp bundle index.js --config rspack.config.js --outdir dist` | ok | unchanged |
| rspack-html-webpack-plugin | rspack | `rkp bundle src/index.js --config rspack.config.js --outdir dist` | ok | unchanged |
| valtio | esbuild+rollup | `rkp bundle src/index.ts --outdir dist --splitting`, `... --outfile dist/index.js --format=cjs`, `... --outfile dist/esm/index.mjs` (config run, #121) | ok (all three, run here after #121) | unchanged |
| rbush | rollup | `rkp bundle index.js --outfile dist/index.js --global-name=RBush --minify` | ok | unchanged |
| d3-array | rollup | `rkp bundle src/index.js --outfile dist/index.js --minify` | ok | unchanged |
| d3-hierarchy | rollup | `rkp bundle src/index.js --outfile dist/index.js --minify` | ok | unchanged |
| preact | rollup | `rkp bundle src/index.js --outfile dist/preact.mjs` (from the `source` and `module` fields of package.json; the build script is code, not a config) | ok (run here) | unchanged |
| signals | esbuild | 27 commands from the `source`, `main`, `module` and `exports` fields of the 9 workspace packages that have a `source` (microbundle reads the same fields), for example `rkp bundle packages/core/src/index.ts --outfile packages/core/dist/signals-core.js --format=cjs` | `packages/core` run here: ok | unchanged |
| underscore | rollup | `rkp bundle modules/index-all.js --outfile dist/index-all.js` | ok | unchanged |
| solid-template-basic | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| solid-template-router | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| solid-template-sass | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| redux-vite-template | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| mui-vite-ts | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| tanstack-query-basic | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vitesse-lite | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| element-plus-starter | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| tanstack-query-simple | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| solid-template-unocss | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-playground-tailwind-v3 | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify` | ok | unchanged |
| vite-react-ts-less | vite | `rkp bundle index.html --config vite.config.ts --outdir dist --minify --public-path=/` (since #124; before it had no --public-path and its page failed in a browser, #125) | ok | unchanged |
| sass-postcss-no-config | vite | `rkp bundle index.html --config vite.config.js --outdir dist --minify` | ok | unchanged |

Not detected (`rkp migrate` exits 1 and says so), 29 projects: mitt, nanoid, immer, clsx, ky, commander, semver, kleur, picocolors, json-server, defu, ufo, pathe, destr, scule, citty, consola, jotai, nanostores, ms, dequal, p-limit, p-map, chalk, ansi-styles, cac, flatbush, htm, uPlot. None has a config or build dependency of esbuild, Vite, webpack, Rollup or rspack; nanoid, immer, pathe, citty, jotai and cac list `vite` or a vite.config only for Vitest, which is what the detection should skip, and kleur builds with its own script.

Browser check of the printed commands: `node tools/run-corpus.mjs --migrate` runs `rkp migrate` on each project after its own build, runs the `build:rkp` commands it prints (the output folder emptied first) and loads the page of an app built that way in headless Chromium, with the project's `expect.page`. All 63 projects: 28 had a command that built, and 27 of the 28 passed (the apps loaded without errors); vite-react-ts-less failed as the note predicts, because its `base` comes from the environment and the page asks for `/user/...` files that the build put elsewhere without `--public-path`. The runner adds the project's `migrateArgs` (`--public-path /`) to the commands, as the note says to, and then it passes.

Left without a command after #121: dayjs (its config is a function of the entry, the file and the name that its build script passes in: running it with nothing gives no input), preact and signals (no Rollup or esbuild config exists: preact builds with its own script, signals only lists esbuild as a dependency) and marked (esbuild API script, #123).
