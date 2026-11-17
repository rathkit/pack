module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.txt$/, loader: 'url-loader', options: { limit: 1000, mimetype: 'text/plain' } }] } };
