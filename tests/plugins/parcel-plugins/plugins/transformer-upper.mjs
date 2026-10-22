import { Transformer } from '@parcel/plugin';

export default new Transformer({
  async loadConfig({ config }) {
    const found = await config.getConfig(['upper.json']);
    return found?.contents ?? { suffix: '!' };
  },
  async transform({ asset, config }) {
    const text = (await asset.getCode()).trim();
    asset.type = 'js';
    asset.setCode(`export default ${JSON.stringify(text.toUpperCase() + config.suffix)};`);
    return [asset];
  },
});
