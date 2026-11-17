import { compression } from 'vite-plugin-compression2';
export default { plugins: [compression({ threshold: 0 })] };
