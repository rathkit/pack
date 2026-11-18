// vendor/cyc.js imports the app's own file, which the entry's chunk holds: a `cyc` chunk would import from the entry's chunk and be imported by it.
// That group is given up (a warning names it); the other one still applies.
export default { mode: 'production', target: 'node', optimization: { splitChunks: { cacheGroups: {
  good: { test: /[\\/]vendor[\\/]ok\.js$/, name: 'good', chunks: 'all', minSize: 0 },
  loop: { test: /[\\/]vendor[\\/]cyc\.js$/, name: 'loop', chunks: 'all', minSize: 0 },
} } } };
