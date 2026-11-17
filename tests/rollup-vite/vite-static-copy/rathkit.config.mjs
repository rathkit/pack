import { viteStaticCopy } from 'vite-plugin-static-copy';
export default { plugins: [viteStaticCopy({ targets: [{ src: 'a.txt', dest: 'copied' }] })] };
