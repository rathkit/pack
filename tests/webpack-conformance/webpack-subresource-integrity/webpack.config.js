const { SubresourceIntegrityPlugin } = require('webpack-subresource-integrity');
const HtmlWebpackPlugin = require('html-webpack-plugin');
module.exports = { mode: 'production', output: { crossOriginLoading: 'anonymous' }, plugins: [new HtmlWebpackPlugin(), new SubresourceIntegrityPlugin()] };
