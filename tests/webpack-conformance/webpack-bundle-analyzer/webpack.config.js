const { BundleAnalyzerPlugin } = require('webpack-bundle-analyzer');
module.exports = { mode: 'production', target: 'node', plugins: [new BundleAnalyzerPlugin({ analyzerMode: 'static', openAnalyzer: false, reportFilename: 'report.html', logLevel: 'silent' })] };
