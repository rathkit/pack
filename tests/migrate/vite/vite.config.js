import { fileURLToPath } from 'node:url';

export default {
  define: { __APP_VERSION__: '"1.2.3"' },
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  build: { outDir: 'dist', sourcemap: true },
};
