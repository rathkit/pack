const HtmlWebpackPlugin = require('html-webpack-plugin');
module.exports = { mode: 'production', plugins: [new HtmlWebpackPlugin({ title: 'Conformance page', filename: 'page.html', meta: { viewport: 'width=device-width' } })] };
