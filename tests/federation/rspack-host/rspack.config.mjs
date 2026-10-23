// An rspack host that loads a remote built by rathkit-pack.
import { ModuleFederationPlugin } from '@module-federation/enhanced/rspack';
import { fileURLToPath } from 'node:url';
const here = fileURLToPath(new URL('.', import.meta.url));
export default {
  mode: 'production', devtool: false, context: here,
  entry: './src/index.js',
  output: { path: `${here}dist`, publicPath: 'http://localhost:4231/', clean: true },
  optimization: { minimize: false },
  plugins: [new ModuleFederationPlugin({ name: 'rhost', remotes: { oapp: 'oapp@http://localhost:4210/remoteEntry.js' }, shared: { react: { singleton: true, requiredVersion: '^19.0.0' } }, dts: false })],
};
