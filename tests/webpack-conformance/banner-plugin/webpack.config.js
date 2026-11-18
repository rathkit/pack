const webpack = require('webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new webpack.BannerPlugin({ banner: 'built by conformance', raw: false })] };
