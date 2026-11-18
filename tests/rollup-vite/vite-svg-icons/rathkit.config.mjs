import { createSvgIconsPlugin } from 'vite-plugin-svg-icons';
import path from 'node:path';
export default { plugins: [createSvgIconsPlugin({ iconDirs: [path.resolve(import.meta.dirname, 'icons')], symbolId: 'icon-[name]' })] };
