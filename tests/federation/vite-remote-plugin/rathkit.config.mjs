// The real @module-federation/vite plugin, run by rathkit-pack's plugin layer (set RATHKIT_REAL_FEDERATION_PLUGIN=1;
// without it the plugin's options are read and the bundler does the work itself).
import { federation } from '@module-federation/vite';

export default {
  base: 'http://localhost:4290/',
  plugins: [
    federation({
      name: 'vapp',
      filename: 'remoteEntry.js',
      exposes: { './Button': './src/Button.js' },
      shared: { react: { singleton: true, requiredVersion: '^19.0.0' } },
      dts: false,
    }),
  ],
};
