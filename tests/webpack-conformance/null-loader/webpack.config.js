module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /ignored\.js$/, loader: 'null-loader' }] } };
