// The configuration imports a file without its extension, which Node cannot follow: it is bundled first, as Vite does.
// The plugin makes a part of a file the way Vue's plugin does: the id ends in `lang.ts`, so the text is TypeScript.
import path from 'node:path';
import { part } from './names';

export default {
  plugins: [
    {
      name: 'part',
      resolveId(id) {
        return id === part ? `${path.resolve('part.sfc')}?vue&type=script&lang.ts` : null;
      },
      load(id) {
        if (!id.includes('part.sfc?')) return null;
        return 'interface Point { x: number }\nexport default function origin(p: Point = { x: 0 }): number { return p.x as number; }\n';
      },
    },
  ],
};
