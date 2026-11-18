import { babel } from '@rollup/plugin-babel';
export default { plugins: [babel({ babelHelpers: 'bundled', babelrc: false, configFile: false, presets: [['@babel/preset-env', { targets: { ie: '11' } }]] })] };
