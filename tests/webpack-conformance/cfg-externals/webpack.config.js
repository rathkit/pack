module.exports = { mode: 'production', target: 'node', externals: { fs: 'commonjs fs', lodash: 'commonjs lodash', 'my-ext': 'var MyExt' }, };
