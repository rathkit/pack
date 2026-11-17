module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.ya?ml$/, loader: 'yaml-loader' }] } };
