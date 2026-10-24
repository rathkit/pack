import { ViteImageOptimizer } from 'vite-plugin-image-optimizer';
export default { plugins: [ViteImageOptimizer({ logStats: false, png: { quality: 50 } })] };
