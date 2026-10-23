// A host built by Vite with the federation plugin, loading a remote built by rathkit-pack.
import { federation } from '@module-federation/vite';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
export default {
  root: here,
  logLevel: 'error',
  configFile: false,
  base: 'http://localhost:4301/',
  build: { outDir: 'dist', target: 'esnext', minify: false },
  plugins: [
    federation({
      name: 'vhost',
      remotes: { oapp: { type: 'var', name: 'oapp', entry: 'http://localhost:4210/remoteEntry.js' } },
      shared: { react: { singleton: true, requiredVersion: '^19.0.0' } },
      dts: false,
    }),
  ],
};
