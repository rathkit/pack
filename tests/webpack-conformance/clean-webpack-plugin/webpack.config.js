const { CleanWebpackPlugin } = require('clean-webpack-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new CleanWebpackPlugin()] };
