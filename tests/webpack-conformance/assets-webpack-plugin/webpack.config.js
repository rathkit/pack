const AssetsPlugin = require('assets-webpack-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new AssetsPlugin({ filename: 'assets.json', path: __dirname + '/dist' })] };
