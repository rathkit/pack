// Both groups match vendor/a.js; the one with the higher priority takes it, and the other takes the rest.
export default { mode: 'production', target: 'node', optimization: { splitChunks: { cacheGroups: {
  everything: { test: /[\\/]vendor[\\/]/, name: 'everything', chunks: 'all', minSize: 0, priority: 1 },
  special: { test: /[\\/]vendor[\\/]a\.js$/, name: 'special', chunks: 'all', minSize: 0, priority: 10 },
} } } };
