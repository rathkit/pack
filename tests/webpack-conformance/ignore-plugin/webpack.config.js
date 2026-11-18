const webpack = require('webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new webpack.IgnorePlugin({ resourceRegExp: /^\.\/locale$/, contextRegExp: /lib$/ })] };
