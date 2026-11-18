const nodeExternals = require('webpack-node-externals');
module.exports = { mode: 'production', target: 'node', externals: [nodeExternals({ modulesDir: __dirname + '/node_modules', allowlist: [] })] };
