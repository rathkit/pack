import viteCompression from 'vite-plugin-compression';
export default { plugins: [viteCompression({ threshold: 0, algorithm: 'gzip', verbose: false })] };
