import { nodeResolve } from '@rollup/plugin-node-resolve';
import polyfillNode from 'rollup-plugin-polyfill-node';
export default { plugins: [polyfillNode(), nodeResolve()] };
