import { Runtime } from '@parcel/plugin';

export default new Runtime({
  apply({ bundle }) {
    return [{ filePath: 'runtime-flag.js', code: 'globalThis.__parcelRuntime = "ran";', isEntry: true, env: { sourceType: 'module' } }];
  },
});
