const ESLintPlugin = require('eslint-webpack-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new ESLintPlugin({ extensions: ['js'], overrideConfigFile: __dirname + '/eslint.config.js', failOnError: false })] };
