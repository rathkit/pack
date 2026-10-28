import Default, { Thing, ThingOptions, A, Dir, lookup, Table, counter, cache, Cache, makeCache } from '../dist/index';
import type { Ext } from 'ext-pkg';
const t: Thing = new Default({ size: 2 });
const area: number = t.area();
const opts: ThingOptions = { size: 1, ext: { id: 'x', added: true } };
const e: Ext = { id: 'y', added: false };
const w: number = window.__lib;
const one: 1 = A;
const dir: Dir = Dir.Up;
const v: 'two' = lookup('beta');
const tb: Table = { alpha: 1, beta: 'two' };
const c: Cache<string> = makeCache<string>();
const got: string | undefined = cache.set('k', 'v').get('k');
export { t, area, opts, e, w, one, dir, v, tb, c, got, counter };
// @ts-expect-error
const bad: number = lookup('beta');
