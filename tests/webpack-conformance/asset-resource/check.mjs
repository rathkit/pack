import { existsSync } from 'node:fs';
import { join } from 'node:path';
// asset/resource writes the file under output.assetModuleFilename.
export default (out) => (existsSync(join(out, 'files', 't.txt')) ? null : 'dist/files/t.txt was not written');
