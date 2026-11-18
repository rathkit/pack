import glsl from 'rollup-plugin-glsl';
export default { plugins: [glsl({ include: '**/*.glsl' })] };
