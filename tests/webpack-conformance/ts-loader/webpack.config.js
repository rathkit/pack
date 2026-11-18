module.exports = { mode: 'production', target: 'node', resolve: { extensions: ['.ts', '.js'] }, module: { rules: [{ test: /\.ts$/, loader: 'ts-loader', options: { transpileOnly: true } }] } };
