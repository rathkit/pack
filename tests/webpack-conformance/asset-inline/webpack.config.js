module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.txt$/, type: 'asset/inline' }] } };
