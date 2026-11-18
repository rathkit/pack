// The modules of `skipped` total less than its minSize, so it makes no file; `forced` is below its minSize too, and enforce ignores it.
export default { mode: 'production', target: 'node', optimization: { splitChunks: { cacheGroups: {
  skipped: { test: /[\\/]vendor[\\/]a\.js$/, name: 'skipped', chunks: 'all', minSize: 100000 },
  forced: { test: /[\\/]vendor[\\/]b\.js$/, name: 'forced', chunks: 'all', minSize: 100000, enforce: true },
} } } };
