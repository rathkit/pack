import { VitePWA } from 'vite-plugin-pwa';
export default { plugins: [VitePWA({ injectRegister: false, manifest: false, workbox: { globPatterns: ['**/*.mjs'] } })] };
