module.exports = { mode: 'production', target: 'node', module: { rules: [{ test: /\.txt$/, oneOf: [{ resourceQuery: /raw/, type: 'asset/source' }, { type: 'asset/inline' }] }] } };
