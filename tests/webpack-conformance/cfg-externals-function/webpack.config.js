module.exports = { mode: 'production', target: 'node', externals: [({ request }, callback) => (request === 'webpack-merge' ? callback(null, 'commonjs ' + request) : callback())] };
