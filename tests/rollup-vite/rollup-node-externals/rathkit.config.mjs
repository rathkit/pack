import { nodeExternals } from 'rollup-plugin-node-externals';
export default { plugins: [nodeExternals({ packagePath: "./package.json" })] };
