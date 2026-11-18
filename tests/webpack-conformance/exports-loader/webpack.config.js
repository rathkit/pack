module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /lib\.js$/, loader: 'exports-loader', options: { type: 'commonjs', exports: 'single hello' } }] } };
