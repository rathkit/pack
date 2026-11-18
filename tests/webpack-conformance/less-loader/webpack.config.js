module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.less$/, use: ['css-loader', 'less-loader'] }] } };
