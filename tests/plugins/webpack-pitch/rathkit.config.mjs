import { fileURLToPath } from 'node:url';
import path from 'node:path';
const here = path.dirname(fileURLToPath(import.meta.url));
export default { module: { rules: [{ test: /\.inject$/, use: [path.join(here, 'inject-loader.cjs')] }] } };
