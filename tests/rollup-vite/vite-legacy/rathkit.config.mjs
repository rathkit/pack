import legacy from '@vitejs/plugin-legacy';
export default { plugins: [legacy({ renderLegacyChunks: false, modernPolyfills: false })] };
