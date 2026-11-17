const webpack = require('webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new webpack.ProvidePlugin({ answer: './answer.js', _: ['./answer.js', 'double'] })] };
