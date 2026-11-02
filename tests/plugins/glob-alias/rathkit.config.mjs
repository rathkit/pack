import path from 'node:path';

// `@` stands for ./src, as in a Vite project; `import.meta.glob` patterns may start with it.
export default {
  resolve: { alias: { '@': path.resolve('src') } },
};
