const { GenerateSW } = require('workbox-webpack-plugin');
module.exports = { mode: 'production', plugins: [new GenerateSW({ swDest: 'sw.js', clientsClaim: true, skipWaiting: true })] };
