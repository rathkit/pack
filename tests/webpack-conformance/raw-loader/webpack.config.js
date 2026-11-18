module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.txt$/, loader: 'raw-loader', options: { esModule: false } }] } };
