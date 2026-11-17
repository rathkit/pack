import url from '@rollup/plugin-url';
export default { plugins: [url({ include: ['**/*.svg'], limit: 10000 })] };
