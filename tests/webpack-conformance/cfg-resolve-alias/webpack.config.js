const path = require('path');
module.exports = { mode: 'production', target: 'node', resolve: { alias: { '@lib': path.resolve(__dirname, 'lib'), exact$: path.resolve(__dirname, 'lib/one.js') } } };
