import { resolve } from 'node:path';

export default {
  build: {
    rollupOptions: {
      input: { main: resolve(import.meta.dirname, 'index.html'), about: resolve(import.meta.dirname, 'about.html') },
    },
  },
};
