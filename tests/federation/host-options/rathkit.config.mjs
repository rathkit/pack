// The options of a ModuleFederationPlugin, as rspack and webpack take them. rathkit-pack reads them and does the work itself.
import { ModuleFederationPlugin } from '@module-federation/enhanced/rspack';

export default {
  plugins: [
    new ModuleFederationPlugin({
      name: 'h2',
      remotes: { r2: 'r2@http://localhost:4250/remoteEntry.js' },
      remoteType: 'module',
      shared: { react: { singleton: true, eager: true }, 'lodash-es/': { singleton: true }, 'lodash-es': { singleton: true } },
      runtimePlugins: [['./src/plugin.js', { note: 'from params' }]],
      dts: false,
    }),
  ],
};
