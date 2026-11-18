const path = require('path');
const webpack = require('webpack');

module.exports = {
  mode: 'production',
  target: 'web',
  entry: './src/main.js',
  output: { path: path.resolve(__dirname, 'dist'), filename: 'main.js' },
  devtool: 'source-map',
  resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
  module: { rules: [{ test: /\.txt$/, type: 'asset/source' }] },
  plugins: [new webpack.DefinePlugin({ __APP_VERSION__: JSON.stringify('1.2.3') })],
};
