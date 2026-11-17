const CompressionPlugin = require('compression-webpack-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new CompressionPlugin({ test: /\.js$/, threshold: 0, minRatio: 1 })] };
