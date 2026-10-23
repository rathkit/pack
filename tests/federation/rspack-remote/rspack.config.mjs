// The same remote built by rspack, for the other bundler to load and to be loaded by.
import { ModuleFederationPlugin } from '@module-federation/enhanced/rspack';
import { fileURLToPath } from 'node:url';
const here = fileURLToPath(new URL('.', import.meta.url));
export default {
  mode: 'production', devtool: false, context: here,
  entry: './src/index.js',
  output: { path: `${here}dist`, publicPath: 'http://localhost:4220/', clean: true },
  optimization: { minimize: false },
  plugins: [new ModuleFederationPlugin({ name: 'rapp', filename: 'remoteEntry.js', exposes: { './Button': './src/Button.js' }, shared: { react: { singleton: true, requiredVersion: '^19.0.0' } }, dts: false })],
};
