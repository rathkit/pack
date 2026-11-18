module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.txt$/, loader: 'file-loader', options: { name: '[name].[ext]' } }] } };
