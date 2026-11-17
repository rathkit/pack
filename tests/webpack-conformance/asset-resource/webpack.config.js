module.exports = { mode: 'production', target: 'node', output: { assetModuleFilename: 'files/[name][ext]' }, module: { rules: [{ test: /\.txt$/, type: 'asset/resource' }] } };
