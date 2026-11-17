module.exports = { mode: 'production', output: { publicPath: '/static/' }, module: { rules: [{ test: /\.txt$/, type: 'asset/resource' }] } };
