module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /lib\.js$/, loader: 'imports-loader', options: { type: 'commonjs', imports: ['single ./dep.js dep'] } }] } };
