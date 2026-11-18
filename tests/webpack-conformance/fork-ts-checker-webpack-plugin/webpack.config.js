const ForkTsCheckerWebpackPlugin = require('fork-ts-checker-webpack-plugin');
module.exports = { mode: 'production', target: 'node', resolve: { extensions: ['.ts', '.js'] }, module: { rules: [{ test: /\.ts$/, loader: 'ts-loader', options: { transpileOnly: true } }] }, plugins: [new ForkTsCheckerWebpackPlugin({ async: false })] };
