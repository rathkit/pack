module.exports = { mode: 'production', target: 'node', devtool: 'source-map', module: { rules: [{ test: /\.js$/, enforce: 'pre', use: ['source-map-loader'] }] } };
