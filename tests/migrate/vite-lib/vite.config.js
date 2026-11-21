import { resolve } from 'node:path';

export default {
  build: {
    outDir: 'lib',
    lib: { entry: resolve(import.meta.dirname, 'src/index.js'), name: 'SampleLib', fileName: 'sample', formats: ['es', 'cjs', 'umd'] },
    rollupOptions: { external: ['vue'], output: { globals: { vue: 'Vue' } } },
  },
};
