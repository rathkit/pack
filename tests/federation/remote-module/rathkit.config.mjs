// The Vite plugin's options; rathkit-pack reads them and does the work itself.
import { federation } from '@module-federation/vite';

export default {
  plugins: [
    federation({
      name: 'r2',
      library: { type: 'module' },
      exposes: { './utils': './src/utils.js' },
      shared: { 'lodash-es/': { singleton: true }, 'lodash-es': { singleton: true } },
      manifest: { fileName: 'federation-manifest.json' },
    }),
  ],
};
