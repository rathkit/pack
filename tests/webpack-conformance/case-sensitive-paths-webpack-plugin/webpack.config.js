const CaseSensitivePathsPlugin = require('case-sensitive-paths-webpack-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new CaseSensitivePathsPlugin()] };
