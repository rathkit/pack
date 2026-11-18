module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /lib\.js$/, loader: 'expose-loader', options: { exposes: 'MyLib' } }] } };
