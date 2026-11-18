# From webpack

`--config webpack.config.js` reads the entry, `resolve.alias`, `module.rules` (loaders and `type: 'asset/*'`) and `plugins` of a
webpack config. `DefinePlugin`, `EnvironmentPlugin`, `ProvidePlugin`, `BannerPlugin`, `IgnorePlugin`, `NormalModuleReplacementPlugin`
and `EntryPlugin` are built in, so `webpack` need not be installed for the config to load; `MiniCssExtractPlugin` is done by the
bundler's own style sheet pipeline. Plugins written against the compiler (hooks, `compilation`, assets, child compilers) and loaders
(pitching, `this.getOptions`, `raw`) run.

Before (`webpack.config.js`; webpack 5.112.2 and webpack-cli 7.3.1, run as `webpack`):

```js
const path = require('path');
const webpack = require('webpack');

module.exports = {
  mode: 'production',
  target: 'web',
  entry: './src/main.js',
  output: { path: path.resolve(__dirname, 'dist'), filename: 'main.js' },
  devtool: 'source-map',
  resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
  module: { rules: [{ test: /\.txt$/, type: 'asset/source' }] },
  plugins: [new webpack.DefinePlugin({ __APP_VERSION__: JSON.stringify('1.2.3') })],
};
```

After (`rkp` 0.3.0-beta.0), with the same config file:

```
rkp bundle src/main.js --config webpack.config.js --platform=browser --minify --sourcemap --outfile dist/main.js
```

Both programs print `hello world 42 aliased from a text file 1.2.3 production`. The alias, the defined constant and the `.txt` rule
all came from the config file (the command line has none of them). `mode: 'production'` is `--minify` plus `--platform=browser`
(which makes `process.env.NODE_ENV` `"production"`); the config's `mode`, `output` and `devtool` are not read.

## What is read from the config, and what is not

| webpack | `rkp` |
| --- | --- |
| `entry` | read (or name the entries on the command line) |
| `output.path`, `output.filename` | `--outfile FILE`, or `--outdir DIR` with `--splitting` |
| `output.filename: '[name].[contenthash].js'` | `--entry-names=[name]-[hash]`, `--chunk-names=` |
| `output.publicPath` | `--public-path` |
| `mode: 'production'` | `--minify --platform=browser` |
| `devtool: 'source-map'`, `'inline-source-map'` | `--sourcemap`, `--sourcemap=inline` |
| `externals` | `--external:NAME` |
| `target: 'node'` | `--platform=node` (the default); `target: 'web'`: `--platform=browser` |
| `resolve.extensions`, `resolve.mainFields`, `resolve.conditionNames` | `--resolve-extensions`, `--main-fields=`, `--conditions=` |
| `devServer` | `rkp serve ENTRY` (`--port`, `--host`, `--https`, `--hmr`) |
| `optimization.splitChunks.cacheGroups` | done with `--splitting`: `test`, `name`, `chunks`, `priority`, `minChunks`, `minSize` and `enforce` decide the chunks; webpack's implicit groups are not added (see `docs/webpack-conformance.md`) |
| `optimization.splitChunks` size settings (`maxSize`), `runtimeChunk` | not done: no webpack runtime, chunks are not cut by size; dynamic `import()` makes chunks with `--splitting` and `--outdir` |

## What is different

- **No config is needed for many projects.** Aliases from `tsconfig.json` `paths`, `.ts`, JSX, CSS (`import './a.css'`), images and
  fonts (copied under a hashed name) work with no loader or plugin: `ts-loader`, `babel-loader` for syntax, `css-loader` with
  `style-loader`, `file-loader` and `url-loader` are not needed.
- **Output names.** A webpack page template that says `<script src="./main.js">` still works: with a webpack config and no
  `--outfile` the default entry's file is `main.js`.
- **Chunk graph internals.** Plugins that depend on webpack's module and chunk graph beyond what the README lists, and
  `SplitChunksPlugin` settings, do not work.
- **webpack's `--watch`.** `rkp bundle ... --watch` builds again when a file the build read changes.
- **Module Federation.** `ModuleFederationPlugin` options are read (`--federation FILE.json`, `federation` in the config); see the README.
