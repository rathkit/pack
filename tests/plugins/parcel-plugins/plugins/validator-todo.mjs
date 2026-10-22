import { Validator } from '@parcel/plugin';
import fs from 'node:fs';
import path from 'node:path';

let seen = 0;
export default new Validator({
  async validate({ asset }) {
    seen++;
    const code = await asset.getCode();
    if (process.env.OUT_DIR) fs.mkdirSync(process.env.OUT_DIR, { recursive: true });
    if (process.env.OUT_DIR) fs.writeFileSync(path.join(process.env.OUT_DIR, 'validated.txt'), `validated ${seen} file(s)\n`);
    return { warnings: code.includes('TODO') ? [{ message: 'a TODO is left in the code' }] : [], errors: [] };
  },
});
