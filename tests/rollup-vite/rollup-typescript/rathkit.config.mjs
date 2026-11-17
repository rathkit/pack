import typescript from '@rollup/plugin-typescript';
export default { plugins: [typescript({ tsconfig: "./tsconfig.json", noEmitOnError: false })] };
