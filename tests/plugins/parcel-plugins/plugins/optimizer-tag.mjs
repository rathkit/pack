import { Optimizer } from '@parcel/plugin';
import { blobToString } from '@parcel/utils';

export default new Optimizer({
  async optimize({ contents, bundle }) {
    return { contents: (await blobToString(contents)) + `\n/* optimized ${bundle.type} */\n` };
  },
});
