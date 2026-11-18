module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.json5$/, loader: 'json5-loader', type: 'javascript/auto' }] } };
