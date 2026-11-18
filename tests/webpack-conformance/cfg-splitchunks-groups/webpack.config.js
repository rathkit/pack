export default { mode: 'production', target: 'node', optimization: { splitChunks: { cacheGroups: { vendors: { test: /[\\/]vendor[\\/]/, name: 'vendors', chunks: 'all', minSize: 0 } } } } };
