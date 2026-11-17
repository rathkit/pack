const webpack = require('webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new webpack.DefinePlugin({ VERSION: JSON.stringify('1.2.3'), 'process.env.FLAG': 'true', 'typeof WINDOW': JSON.stringify('undefined'), NESTED: { A: '1' } })] };
