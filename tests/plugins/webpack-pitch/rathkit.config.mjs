import path from 'node:path';
const here = path.dirname(new URL(import.meta.url).pathname);
export default { module: { rules: [{ test: /\.inject$/, use: [path.join(here, 'inject-loader.cjs')] }] } };
