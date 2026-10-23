// A remote built by Vite with the federation plugin, for a host built here to load.
import { federation } from '@module-federation/vite';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
export default {
  root: here,
  logLevel: 'error',
  configFile: false,
  base: 'http://localhost:4290/',
  build: { outDir: 'dist', target: 'esnext', minify: false, rollupOptions: { input: 'src/main.js' } },
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
