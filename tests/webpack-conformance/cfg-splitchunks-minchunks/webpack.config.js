// minChunks counts the chunks that reach a module: vendor/two.js has two, vendor/one.js one.
export default { mode: 'production', target: 'node', optimization: { splitChunks: { cacheGroups: {
  common: { test: /[\\/]vendor[\\/]/, name: 'common', chunks: 'async', minChunks: 2, minSize: 0 },
} } } };
