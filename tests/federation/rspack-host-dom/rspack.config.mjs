// An rspack host that renders a component of a remote built by rathkit-pack; React and React DOM are shared.
import { ModuleFederationPlugin } from '@module-federation/enhanced/rspack';
import { fileURLToPath } from 'node:url';
const here = fileURLToPath(new URL('.', import.meta.url));
export default {
  mode: 'production', devtool: false, context: here,
  entry: './src/index.js',
  output: { path: `${here}dist`, publicPath: 'http://localhost:4281/', clean: true },
  optimization: { minimize: false },
  plugins: [new ModuleFederationPlugin({
    name: 'rdomhost',
    remotes: { domapp: 'domapp@http://localhost:4280/remoteEntry.js' },
    shared: { react: { singleton: true, requiredVersion: '^19.0.0' }, 'react-dom': { singleton: true, requiredVersion: '^19.0.0' }, 'react-dom/': { singleton: true } },
    dts: false,
  })],
};
