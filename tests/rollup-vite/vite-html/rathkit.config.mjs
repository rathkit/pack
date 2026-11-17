import { createHtmlPlugin } from 'vite-plugin-html';
export default { plugins: [createHtmlPlugin({ inject: { data: { title: 'Titled' } } })] };
