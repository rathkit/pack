import Dflt, { Svc, Plain, Level, Tools } from './svc.ts';
import { ready, value } from './waiter.ts';
import { again, sync } from './reexport.ts';

const out = (...a: unknown[]) => console.log(...a.map((x) => (typeof x === 'object' && x !== null ? JSON.stringify(x) : String(x))));
out('decorators', (Svc as any).marked, (Dflt as any).marked, new Svc().run(), new Dflt().hi(), (Svc as any).fields, new Plain().value);
out('enum and namespace', Level.High, Level[1], Tools.twice(4));
out('waited', ready, value, again, sync);
const lazy = await import('./lazy.ts');
out('dynamic', lazy.lazy, lazy.default);
const late = await Promise.resolve('main-waited');
out('main', late);
