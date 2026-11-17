module.exports = { mode: 'production', module: { rules: [{ test: /\.css$/, use: ['style-loader', 'css-loader'] }] } };
