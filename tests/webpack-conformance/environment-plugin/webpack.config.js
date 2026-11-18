const webpack = require('webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new webpack.EnvironmentPlugin({ GREETING: 'hello', COUNT: 3 })] };
