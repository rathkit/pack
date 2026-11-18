// 'initial' takes only what the entry reaches without import(), 'async' only what import() alone reaches.
export default { mode: 'production', target: 'node', optimization: { splitChunks: { cacheGroups: {
  first: { test: /[\\/]vendor[\\/]i-/, name: 'first', chunks: 'initial', minSize: 0 },
  later: { test: /[\\/]vendor[\\/]a-/, name: 'later', chunks: 'async', minSize: 0 },
  wrongKind: { test: /[\\/]vendor[\\/]s-/, name: 'wrong-kind', chunks: 'async', minSize: 0 },
  wrongKind2: { test: /[\\/]vendor[\\/]a-/, name: 'wrong-kind-2', chunks: 'initial', minSize: 0, priority: 5 },
} } } };
