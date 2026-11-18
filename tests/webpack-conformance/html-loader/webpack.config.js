module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.html$/, loader: 'html-loader', options: { esModule: false } }, { test: /\.png$/, type: 'asset/resource' }] } };
