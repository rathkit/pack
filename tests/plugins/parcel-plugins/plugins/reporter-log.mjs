import { Reporter } from '@parcel/plugin';
import fs from 'node:fs';
import path from 'node:path';

const events = [];
export default new Reporter({
  async report({ event }) {
    events.push(event.type);
    if (event.type === 'buildSuccess' && process.env.OUT_DIR) {
      const names = event.bundleGraph.getBundles().map((b) => b.type.replace(/^[mc]js$/, 'js')).sort();
      fs.writeFileSync(path.join(process.env.OUT_DIR, 'hooks.log'), `${events.join(',')} bundles: ${names.join('+')}\n`);
    }
  },
});
