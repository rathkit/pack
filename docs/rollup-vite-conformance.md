# Rollup and Vite conformance

Which of the plugins people use most with Rollup and with Vite run when `rkp bundle` is given their config (`--config FILE`, or a
`rathkit.config.*` or `vite.config.*` found at the root). Every row below has a fixture under
[`tests/rollup-vite/`](../tests/rollup-vite/) named in the last column. `node tools/run-rollup-vite-conformance.mjs` builds each one with the real
package from `test-deps/` and, unless the row says otherwise, runs the output with Node and compares what it prints (install with
`pnpm install --dir test-deps --frozen-lockfile --ignore-scripts`; without it the suite is skipped). Some fixtures check the files the build wrote
instead, or as well.

Status: **works** (the fixture passes as described), **partly** (it passes, and the Notes say what is not covered), **no** (see the gaps at the end).
A plugin that only has hooks for `vite serve` (`apply: 'serve'`) is checked for what it does in a build: it must not break one; its
serve behavior is covered by `tools/run-hmr.mjs` and the serve tests, not here.

The two lists are my judgment of the most used plugins, from what the package registry downloads and what the Rollup and Vite guides
name, as far as I know it; the order is rough. A plugin not listed may work.

## Rollup: 53 plugins

| # | Plugin | Status | What the fixture does | Fixture |
|---|---|---|---|---|
| 1 | `@rollup/plugin-node-resolve` | works | a bare import from `node_modules` | `rollup-node-resolve` |
| 2 | `@rollup/plugin-commonjs` | works | a `.cjs` module; redundant under rkp, which reads CommonJS itself, but a shared config runs | `rollup-commonjs` |
| 3 | `@rollup/plugin-json` | works | default and named imports | `rollup-json` |
| 4 | `@rollup/plugin-typescript` | works | `tsconfig.json`, type annotations | `rollup-typescript` |
| 5 | `@rollup/plugin-babel` | works | `@babel/preset-env` to ES5 | `rollup-babel` |
| 6 | `@rollup/plugin-terser` | works | comments are dropped | `rollup-terser` |
| 7 | `@rollup/plugin-replace` | works | `preventAssignment`, `process.env.NODE_ENV` | `rollup-replace` |
| 8 | `@rollup/plugin-alias` | works | `entries` with a path | `rollup-alias` |
| 9 | `@rollup/plugin-url` | works | a data URL under the limit | `rollup-url` |
| 10 | `@rollup/plugin-image` | works | a PNG becomes a data URL | `rollup-image` |
| 11 | `@rollup/plugin-dsv` | works | CSV rows | `rollup-dsv` |
| 12 | `@rollup/plugin-yaml` | works |  | `rollup-yaml` |
| 13 | `@rollup/plugin-wasm` | works | inlined module, instantiated with `imports` | `rollup-wasm` |
| 14 | `@rollup/plugin-strip` | works | `console.debug`, `assert.*` | `rollup-strip` |
| 15 | `@rollup/plugin-inject` | works | a free name imported from a file | `rollup-inject` |
| 16 | `@rollup/plugin-virtual` | works |  | `rollup-virtual` |
| 17 | `@rollup/plugin-run` | works | runs the written bundle | `rollup-run` |
| 18 | `@rollup/plugin-sucrase` | works | TypeScript | `rollup-sucrase` |
| 19 | `@rollup/plugin-swc` | works | TypeScript | `rollup-swc` |
| 20 | `@rollup/plugin-esm-shim` | works | `__filename` in an ES module | `rollup-esm-shim` |
| 21 | `@rollup/plugin-html` | works | `index.html` is written | `rollup-html` |
| 22 | `@rollup/plugin-multi-entry` | works | an `options` hook that replaces `input` | `rollup-multi-entry` |
| 23 | `@rollup/plugin-dynamic-import-vars` | works | `import(`./locales/${l}.js`)` | `rollup-dynamic-import-vars` |
| 24 | `@rollup/plugin-graphql` | works |  | `rollup-graphql` |
| 25 | `@rollup/plugin-legacy` | works | a script that sets a global | `rollup-legacy` |
| 26 | `rollup-plugin-postcss` | works | `inject: false`, the CSS as a string | `rollup-postcss` |
| 27 | `rollup-plugin-terser` | works |  | `rollup-plugin-terser` |
| 28 | `rollup-plugin-typescript2` | works | `check: false` | `rollup-typescript2` |
| 29 | `rollup-plugin-esbuild` | works | TypeScript | `rollup-esbuild` |
| 30 | `rollup-plugin-visualizer` | works | `stats.html` is written | `rollup-visualizer` |
| 31 | `rollup-plugin-copy` | works | a file is copied | `rollup-copy` |
| 32 | `rollup-plugin-delete` | works | a folder's files are removed at `buildStart` | `rollup-delete` |
| 33 | `rollup-plugin-dts` | works | a `.d.ts` entry | `rollup-dts` |
| 34 | `rollup-plugin-peer-deps-external` | works | a peer dependency stays an import | `rollup-peer-deps-external` |
| 35 | `rollup-plugin-node-polyfills` | partly | loads and runs. On a Node build `path` is the built-in module; on a browser build (`--platform-browser`) it is the plugin's, checked by hand | `rollup-node-polyfills` |
| 36 | `rollup-plugin-license` | works | `banner` | `rollup-license` |
| 37 | `rollup-plugin-sourcemaps` | works | a `//# sourceMappingURL` next to a dependency | `rollup-sourcemaps` |
| 38 | `rollup-plugin-svelte` | works | a `.svelte` component | `rollup-svelte` |
| 39 | `rollup-plugin-filesize` | works |  | `rollup-filesize` |
| 40 | `rollup-plugin-scss` | works | `output: false`, the CSS as a string | `rollup-scss` |
| 41 | `rollup-plugin-string` | works |  | `rollup-string` |
| 42 | `rollup-plugin-import-css` | works | a style sheet imported without a name is emitted | `rollup-import-css` |
| 43 | `rollup-plugin-node-externals` | works | dependencies stay imports | `rollup-node-externals` |
| 44 | `rollup-plugin-preserve-shebang` | works |  | `rollup-preserve-shebang` |
| 45 | `rollup-plugin-cleanup` | works | comments are removed | `rollup-cleanup` |
| 46 | `rollup-plugin-polyfill-node` | partly | loads and runs. On a Node build `path` is the built-in module; on a browser build (`--platform-browser`) it is the plugin's, checked by hand | `rollup-polyfill-node` |
| 47 | `rollup-plugin-web-worker-loader` | works | `web-worker:` imports | `rollup-web-worker-loader` |
| 48 | `rollup-plugin-glsl` | works |  | `rollup-glsl` |
| 49 | `rollup-plugin-analyzer` | works |  | `rollup-analyzer` |
| 50 | `@mdx-js/rollup` | works | an `.mdx` file as a component | `rollup-mdx` |
| 51 | `rollup-plugin-styles` | works | `mode: 'extract'` | `rollup-styles` |
| 52 | `rollup-plugin-banner2` | works |  | `rollup-banner2` |
| 53 | `@svgr/rollup` | works | an SVG as a React component | `rollup-svgr` |

## Vite: 50 plugins

Vite's own plugins (the CSS, asset, JSON and `import.meta.glob` ones) are built in and covered by the corpus and the CSS and asset suites.

| # | Plugin | Status | What the fixture does | Fixture |
|---|---|---|---|---|
| 1 | `@vitejs/plugin-react` | works | JSX, automatic runtime | `vite-react` |
| 2 | `@vitejs/plugin-react-swc` | works | JSX | `vite-react-swc` |
| 3 | `@vitejs/plugin-vue` | works | a single-file component | `vite-vue` |
| 4 | `@vitejs/plugin-vue-jsx` | works |  | `vite-vue-jsx` |
| 5 | `@vitejs/plugin-legacy` | partly | the plugin is skipped with a warning (one bundle, no `nomodule` build); `--target es5` is the replacement | `vite-legacy` |
| 6 | `@vitejs/plugin-basic-ssl` | works | serve only; a build is unaffected | `vite-basic-ssl` |
| 7 | `vite-plugin-svgr` | works | `?react` imports | `vite-svgr` |
| 8 | `vite-tsconfig-paths` | works | `paths` in `tsconfig.json` | `vite-tsconfig-paths` |
| 9 | `vite-plugin-pwa` | works | `sw.js` is written (workbox) | `vite-pwa` |
| 10 | `vite-plugin-inspect` | works | serve only | `vite-inspect` |
| 11 | `vite-plugin-checker` | partly | `typescript: false`; the checkers run only in serve | `vite-checker` |
| 12 | `vite-plugin-dts` | works | `main.d.ts` is written | `vite-dts` |
| 13 | `vite-plugin-compression` | works | `.gz` written | `vite-compression` |
| 14 | `vite-plugin-compression2` | works | `.gz` written (the plugin adds a plugin to the resolved config) | `vite-compression2` |
| 15 | `vite-plugin-static-copy` | works |  | `vite-static-copy` |
| 16 | `vite-plugin-html` | works | `inject.data` in an HTML entry | `vite-html` |
| 17 | `vite-plugin-node-polyfills` | partly | loads and runs on a Node build; a browser build (`--platform-browser`) stops with `No matching export in 'path'` | `vite-node-polyfills` |
| 18 | `vite-plugin-wasm` | works | the `.wasm` file is emitted; the run is not checked (the plugin fetches it, a browser feature) | `vite-wasm` |
| 19 | `vite-plugin-mkcert` | works | serve only | `vite-mkcert` |
| 20 | `vite-plugin-solid` | works | JSX for Solid | `vite-solid` |
| 21 | `@sveltejs/vite-plugin-svelte` | works | a component | `vite-svelte` |
| 22 | `vite-plugin-externalize-deps` | works | `build.rollupOptions.external` from a `config` hook | `vite-externalize-deps` |
| 23 | `vite-plugin-lib-inject-css` | works |  | `vite-lib-inject-css` |
| 24 | `vite-plugin-css-injected-by-js` | works | the CSS is in the script | `vite-css-injected-by-js` |
| 25 | `vite-plugin-handlebars` | works | an HTML entry | `vite-handlebars` |
| 26 | `vite-plugin-commonjs` | works |  | `vite-commonjs` |
| 27 | `vite-plugin-environment` | works | `process.env.X` | `vite-environment` |
| 28 | `unplugin-auto-import` | works |  | `vite-auto-import` |
| 29 | `unplugin-vue-components` | works | a component found in a folder | `vite-vue-components` |
| 30 | `unplugin-icons` | works | `compiler: 'raw'`, an `~icons/` import | `vite-icons` |
| 31 | `unocss` | works | `virtual:uno.css` | `vite-unocss` |
| 32 | `@tailwindcss/vite` | works | `@import "tailwindcss"` | `vite-tailwind` |
| 33 | `vite-plugin-svg-icons` | works | `virtual:svg-icons-register` | `vite-svg-icons` |
| 34 | `vite-plugin-restart` | works | serve only | `vite-restart` |
| 35 | `vite-plugin-remove-console` | works |  | `vite-remove-console` |
| 36 | `vite-plugin-glsl` | works | a `transform` hook with a glob `filter` on a file type the bundler has no loader for | `vite-glsl` |
| 37 | `vite-plugin-ejs` | works | an HTML entry | `vite-ejs` |
| 38 | `vite-plugin-vue-devtools` | works | serve only | `vite-vue-devtools` |
| 39 | `vite-plugin-sitemap` | works | `sitemap.xml` is written | `vite-sitemap` |
| 40 | `vite-plugin-qrcode` | works | serve only | `vite-qrcode` |
| 41 | `vite-plugin-node-stdlib-browser` | partly | loads and runs. On a Node build `path` is the built-in module; on a browser build (`--platform-browser`) it is the plugin's, checked by hand | `vite-node-stdlib-browser` |
| 42 | `vite-plugin-pages` | works | `~pages` | `vite-pages` |
| 43 | `vite-plugin-require` | works | `require()` in a module | `vite-require` |
| 44 | `@originjs/vite-plugin-federation` | partly | loads and builds with no remotes; see the federation suite for module federation | `vite-federation-originjs` |
| 45 | `vite-imagetools` | works | `?w=8&format=webp` | `vite-imagetools` |
| 46 | `vite-plugin-image-optimizer` | works |  | `vite-image-optimizer` |
| 47 | `unplugin-fonts` | works | local fonts, inline `@font-face` | `vite-unplugin-fonts` |
| 48 | `vite-plugin-vuetify` | works | with `@vitejs/plugin-vue` | `vite-vuetify` |
| 49 | `vite-plugin-vue-layouts` | works | `virtual:generated-layouts` | `vite-vue-layouts` |
| 50 | `vite-plugin-banner` | works |  | `vite-banner` |

## Found and fixed while writing the suite

- `this.getModuleInfo(id)` returned null while a `transform` hook ran, so `@rollup/plugin-commonjs` stopped with "Cannot read properties of null (reading 'meta')".
- An `options` hook that replaces `input` (`@rollup/plugin-multi-entry`) was not honored for the entries or for `buildStart`.
- A chunk's `modules` listed modules that were shaken out entirely; Rollup's does not, and `rollup-plugin-import-css` and `rollup-plugin-styles` use that to find the style sheets nothing imports by name.
- The config a `config` hook receives lacked `build`, `server`, `preview` and `optimizeDeps`, which plugins write to without checking (`@vitejs/plugin-basic-ssl`), and the resolved config lacked `preview` and `createResolver` (`@tailwindcss/vite`).
- A plugin that adds a plugin to the resolved config in `configResolved` (`vite-plugin-compression2`) had it ignored.
- A `config` hook's `build.rollupOptions.external` was ignored (`vite-plugin-externalize-deps`).
- A hook `filter` given as a glob (`**/*.glsl`) was matched as a substring.
- A module a plugin loads for a file type the bundler has a loader for, whose code opens with an assignment (`URL = globalThis.URL`), was read as that file type (`vite-plugin-wasm`).

## Not supported, and not in the lists

Each of these is a candidate for an issue:
1. `vite-plugin-singlefile`: an HTML entry's page is not in the `bundle` that `generateBundle` receives, so the plugin cannot inline the script into it.
2. `vite-plugin-md` (Markdown as a Vue component; not installed in `test-deps` any more, because its dependencies carry known advisories and no fixture uses it, so this was checked once and is not rerun): `plugin-vue`'s `?vue&type=script&setup=true&lang.ts` request for an `.md` file is read as the file's own type, and the output is a string.
3. `vite-plugin-babel` imports `@babel/core` as a default export, which Babel 8 does not have; it fails to load in Node too, so there is no fixture (`@rollup/plugin-babel` is covered).
4. `vite-plugin-top-level-await` (not installed in `test-deps` any more, for the same reason as `vite-plugin-md`) fails with "missing field `type`" under Vite 8 itself with the pinned `@swc/core`; top-level await needs no plugin here (it is native, for the esm format).
5. `@vitejs/plugin-legacy` is skipped (one bundle, no `nomodule` pass).
6. `vite-plugin-node-polyfills` on a browser build (`--platform-browser`): `import path from 'path'` stops with "No matching export in 'path'", where the other three Node polyfill plugins supply the module.
