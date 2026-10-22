import { Compressor } from '@parcel/plugin';
import { Readable } from 'node:stream';

export default new Compressor({
  async compress({ stream }) {
    const parts = [];
    for await (const part of stream) parts.push(Buffer.from(part));
    return { stream: Readable.from([Buffer.from(Buffer.concat(parts).toString('utf8').length + ' chars')]), type: 'size' };
  },
});
