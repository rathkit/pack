import vue from '@vitejs/plugin-vue';
import Components from 'unplugin-vue-components/vite';
export default { plugins: [vue(), Components({ dirs: ['comps'], dts: false })] };
