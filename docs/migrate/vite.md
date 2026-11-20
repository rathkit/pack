# From Vite

The page is the entry, as in Vite: `rkp bundle index.html` builds the module scripts the page points at, writes the page again with
the built files in place, and copies `public/`. A Vite config is read with `--config` for `define`, `resolve.alias` and its plugins
(Vite and Rollup plugins run, `enforce`, `apply`, `config` and `configResolved` among the hooks).

Before (`vite.config.js`; Vite 8.3.5):

```js
import { fileURLToPath } from 'node:url';

export default {
  define: { __APP_VERSION__: '"1.2.3"' },
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  build: { outDir: 'dist', sourcemap: true },
};
```

with `vite build` (and `vite` for the server). The page has `<script type="module" src="/src/main.js">`, and the program uses
`import.meta.env.MODE`, `import.meta.env.VITE_API` (from `.env`), `./banner.txt?raw` and `@/label.js`.

After (`rkp` 0.3.0-beta.0), with the same config file and the same sources:

```
rkp bundle index.html --config vite.config.js --outdir dist --minify --sourcemap
```

The built program prints the same line as Vite's: `hello world 42 aliased from a text file 1.2.3 production https://api.example.test`
(run in Node with a stand-in for `document`, as Vite's output wants a page). The page is written with
`<script type="module" src="./main.js">`.

The development server takes the script as its entry, and serves the project's `index.html` with that script pointed at the build in memory:

```
rkp serve src/main.js --config vite.config.js --hmr
```

(Tried: the page came back with `<script type="module" src="/bundle.js">` and the reload client, and `/bundle.js` was the build.)

## What is read from `vite.config`, and what is not

Read: `define`, `resolve.alias`, `plugins`, and what plugins' `config` hooks set. Not read, so give them on the command line:

| Vite | `rkp` |
| --- | --- |
| `build.outDir` | `--outdir DIR` (required for a page: it is written next to its scripts) |
| `build.minify` (on by default in Vite) | `--minify` |
| `build.sourcemap` | `--sourcemap`, `--sourcemap=inline` |
| `build.target` | `--target=es2020,chrome80`, or a `browserslist` |
| `base` | `--public-path=/app/` (also `import.meta.env.BASE_URL`). `rkp migrate` takes the value when the config writes it, or names a constant of the config or of a file it imports, or a variable (`process.env.NAME`, `import.meta.env.NAME`, `loadEnv`) that a `.env`, `.env.local`, `.env.production` or `.env.production.local` file of the project sets; when it cannot know, its note names the variable and asks for the value. A `base` of `/` is kept as `--public-path=/`; without a `base`, nothing is added |
| `build.lib` | one `rkp bundle ENTRY --outfile OUTDIR/FILE` for each format, with the file name Vite gives (`fileName`, or the package name; `.js` or `.mjs`, `.cjs`, `.umd.cjs`, `.iife.js`), `--external` for `rollupOptions.external` and `--global` for `output.globals`. `umd` is an iife with `--global-name` (no wrapper that `require` reads). Several entries are one `--format esm,cjs --outdir` command, named after the entries |
| `build.rollupOptions.input` (several pages) | `rkp bundle index.html about.html ... --outdir dist`. Pages with the same file name in different folders (`nested/index.html`) would overwrite one another in the output folder (rkp writes each page under its file name), so the command has `index.html` only and the note says so |
| `mode`, `--mode staging` | `--mode staging` |
| `envDir`, `envPrefix` | `--env-dir DIR`, `--env-prefix PREFIX` |
| `build.lib` | `--format esm,cjs`, see the README's library mode |
| `server.port`, `server.host`, `server.https` | `--port`, `--host`, `--https` |

## What is different

- **File names.** Output is `dist/main.js`, named after the entry, not `dist/assets/index-HASH.js`. For a hash in the name use
  `--entry-names=[name]-[hash]` (and `--chunk-names=`, `--asset-names=`).
- **Vite's own defaults** (minify, a polyfill for `modulepreload`, CSS code splitting) are not repeated: only what the command line
  names is done.
- **`import.meta.env`**, `import.meta.glob`, `?raw`, `?url`, `?inline`, `?worker`, `?init` for WebAssembly, `.env` files and
  `*.module.css` work as in Vite (README: "Vite's import forms").
- **`vite preview`.** Not there; serve the output folder with a static file server.
- **`process.env.NODE_ENV`.** `--platform=browser` makes it `"production"`; `--define:process.env.NODE_ENV='"development"'` changes it.
- **SSR** (`build.ssr`) is not supported.
- **Plugins.** Those checked in CI are listed in the README (`vite-plugin-svgr`, `vite-imagetools`, `@vitejs/plugin-vue` and
  `@sveltejs/vite-plugin-svelte` under `--hmr`, among them). A plugin not listed may work; a report with its name is welcome.
