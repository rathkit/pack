# test-deps

The packages that Pack's differential and interop tests load: real libraries and compilers to bundle, and the bundlers and
plugins Pack is tested to work next to (rspack and Vite for module federation and the plugin cases). Nothing here is built
into `rkp`, and nothing here is a benchmark. The benchmark and the tools it compares Pack with live in
[rathkit/pack-bench](https://github.com/rathkit/pack-bench), and run only in a container.

Install from the lockfile, with no install scripts:

```sh
pnpm install --dir test-deps --frozen-lockfile --ignore-scripts
```

`RATHKIT_TEST_DEPS` names another folder with a `node_modules` (for example a copy inside a container). The folder is a
leaf at the repository root on purpose: a `node_modules` or `package.json` above `tests/` would change what the fixtures test.

| Script | Uses |
|---|---|
| `tools/run-assets.mjs` | `@rollup/plugin-url`, `url-loader`, `file-loader`, `@svgr/rollup`, `@svgr/webpack`, `vite-plugin-svgr`, `vite-imagetools`, `vite-plugin-image-optimizer` (with `sharp`, `svgo`), `unplugin-fonts`, `vite-plugin-webfont-dl`, `rollup-plugin-copy`, `@fontsource/roboto-mono`, `react`, `react-dom`, `vite` |
| `tools/run-css.mjs` | `sass`, `less`, `stylus`, `postcss`, `autoprefixer`, `postcss-nested` |
| `tools/run-federation.mjs` | `@rspack/core`, `@module-federation/enhanced`, `@module-federation/vite`, `vite`, `typescript`, `react`, `react-dom`, `lodash-es` |
| `tools/run-hmr.mjs` | `react`, `react-dom`, `react-refresh`, `rxjs`, `lodash-es`, `three`, `vue`, `@vitejs/plugin-vue`, `svelte`, `@sveltejs/vite-plugin-svelte`, `vite` |
| `tools/run-dts.mjs`, `tools/test-npm-wrapper.mjs` | `typescript` |
| `tools/run-target.mjs`, `tools/check-estree.mjs` | `acorn` |
| `tools/diff-semver.mjs` | `semver` |
| `tools/diff-real.mjs` | the packages in `node_modules` at the top level: `typescript`, `acorn`, `semver`, `rxjs`, `react` run |
| `tools/run-jsx.mjs`, `tools/diff-ts.mjs`, `tests/fixtures.rs` | `ts5` (`TS_MODULE`) |
