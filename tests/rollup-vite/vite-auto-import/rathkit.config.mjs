import AutoImport from 'unplugin-auto-import/vite';
export default { plugins: [AutoImport({ imports: [{ 'lodash-es': ['camelCase'] }], dts: false })] };
