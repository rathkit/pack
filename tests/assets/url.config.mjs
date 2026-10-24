import url from '@rollup/plugin-url';
export default { plugins: [url({ limit: 10, include: ['**/*.png', '**/*.svg', '**/*.woff2'], fileName: '[name]-[hash][extname]' })] };
