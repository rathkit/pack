import Pages from 'vite-plugin-pages';
import vue from '@vitejs/plugin-vue';
export default { plugins: [vue(), Pages({ dirs: 'pages' })] };
