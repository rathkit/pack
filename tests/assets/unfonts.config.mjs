import Unfonts from 'unplugin-fonts/vite';
export default { plugins: [Unfonts({ custom: { families: [{ name: 'Mono', src: './fonts/*.woff2' }], display: 'swap' } })] };
