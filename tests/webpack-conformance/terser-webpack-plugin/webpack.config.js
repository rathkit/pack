const TerserPlugin = require('terser-webpack-plugin');
module.exports = { mode: 'production', target: 'node', optimization: { minimize: true, minimizer: [new TerserPlugin({ extractComments: false, terserOptions: { compress: { drop_console: false }, mangle: true } })] } };
