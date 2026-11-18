const CircularDependencyPlugin = require('circular-dependency-plugin');
module.exports = { mode: 'production', target: 'node', plugins: [new CircularDependencyPlugin({ exclude: /node_modules/, failOnError: false })] };
