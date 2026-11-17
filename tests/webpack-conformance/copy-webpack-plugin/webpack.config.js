const CopyPlugin = require('copy-webpack-plugin');
module.exports = { mode: 'production', plugins: [new CopyPlugin({ patterns: [{ from: 'static', to: 'public' }, { from: 'one.txt', to: 'renamed.txt' }] })] };
