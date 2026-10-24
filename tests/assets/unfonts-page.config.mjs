import Unfonts from 'unplugin-fonts/vite';
export default { plugins: [Unfonts({ google: { families: [{ name: 'Roboto', styles: 'wght@400;700' }], display: 'swap', preconnect: true }, custom: { families: [{ name: 'Mono', src: './fonts/*.woff2' }], display: 'swap', preload: true } })] };
