const { merge } = require('webpack-merge');
const webpack = require('webpack');
const common = { mode: 'production', target: 'node', plugins: [new webpack.DefinePlugin({ A: '1' })] };
module.exports = merge(common, { plugins: [new webpack.DefinePlugin({ B: '2' })] });
