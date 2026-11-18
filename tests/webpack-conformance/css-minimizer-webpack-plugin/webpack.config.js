const MiniCssExtractPlugin = require('mini-css-extract-plugin');
const CssMinimizerPlugin = require('css-minimizer-webpack-plugin');
module.exports = { mode: 'production', plugins: [new MiniCssExtractPlugin({ filename: 'styles.css' })], optimization: { minimizer: ['...', new CssMinimizerPlugin()] }, module: { rules: [{ test: /\.css$/, use: [MiniCssExtractPlugin.loader, 'css-loader'] }] } };
