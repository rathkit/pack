import { describe, configure, Impl, Color, overload, Shapes, Merged, UtilHelper, Helper, main, more, more2, Item, Options, makeHidden, makeExt, Ext, Config, Mode, Base, frozen } from '../dist/index';
const o: Options = { name: 'n' };
const s: string = describe(o);
const i: Item = { kind: 'index' };
const c: Config = configure(o);
const m: Mode = c.mode;
const h = makeHidden().secret + 1;
const impl = new Impl().get();
const col: Color = Color.Green;
const ov: number = overload(1);
const sh: Shapes.Circle = Shapes.unit;
const mg: Merged = { a: 1 };
const mgx: string = Merged.extra;
const uh: UtilHelper = new UtilHelper();
const hh: Helper = new Helper();
const mm: more2.MoreItem = main({ kind: 'util', from: { kind: 'types' } });
const mu: more2.util.Item = { kind: 'util', from: { kind: 'types' } };
const e: Ext = makeExt('x');
const mo: string = more + more2.more;
const f: 1 = frozen.a;
const b: Base<number> = new Impl();
export { s, i, m, h, impl, col, ov, sh, mg, mgx, uh, hh, mm, mu, e, mo, f, b };
// Wrong uses are errors:
// @ts-expect-error
const bad: number = describe(o);
// @ts-expect-error
const bad2: Item = { kind: 'util', from: { kind: 'types' } };
