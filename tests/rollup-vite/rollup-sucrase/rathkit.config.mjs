import sucrase from '@rollup/plugin-sucrase';
export default { plugins: [sucrase({ exclude: ['node_modules/**'], transforms: ['typescript'] })] };
