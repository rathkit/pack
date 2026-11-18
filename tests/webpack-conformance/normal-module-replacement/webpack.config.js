const webpack = require('webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new webpack.NormalModuleReplacementPlugin(/env\.dev\.js$/, './env.prod.js')] };
