const { WebpackManifestPlugin } = require('webpack-manifest-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new WebpackManifestPlugin({ fileName: 'asset-manifest.json' })] };
