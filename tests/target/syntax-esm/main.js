import { helper } from './helper.js';
export const answer = helper(41);
const log = (...a) => console.log(...a.map((x) => (typeof x === 'object' && x !== null ? JSON.stringify(x) : String(x))));
let calls = 0;
const side = (v) => { calls++; return v; };
const o = { a: { b: { c: 5, f() { return this === o.a.b ? 'this-ok' : 'this-bad'; } } }, n: null, u: undefined, z: 0, s: '', arr: [10, 20], fn: () => 'fn' };

// ?.
log('chain1', o?.a?.b?.c, o.n?.x, o.u?.x.y.z, o.missing?.deep, o?.['a']?.['b'].c);
log('chain2', o.a.b.f?.(), o.a?.b?.f(), o.a.b?.f?.(), o.nope?.(), o.n?.(), o.a.nope?.(), o.arr?.[1], o.arr?.[side(0)]);
log('chain3', (o.n?.x)?.toString === undefined ? 'unreached' : 'x', 'calls=' + calls);
log('chain4', side(null)?.[side('k')], calls);
const f = () => o;
log('chain5', f()?.a?.b?.c, f?.()?.a.b.c);
log('chain6', delete o.n?.x, delete o.a?.b.c, o.a.b.c, delete o.nope?.x.y);
log('chain7', typeof o?.fn?.(), o.fn?.(), (o?.a).b.c);
class K { m() { return super.toString?.() === '[object Object]'; } x() { return this?.y?.z; } }
log('chain8', new K().m(), new K().x());
log('chain9', o?.a.b.f(), (o?.a.b).f());

// ??
log('nullish', o.n ?? 'd1', o.u ?? 'd2', o.z ?? 'd3', o.s ?? 'd4', side(null) ?? side('rhs'), calls);
log('nullish2', (o.n ?? o.u) ?? 'both', o.n ?? (o.u, 'seq'), (0 || null) ?? 'x');

// logical assignment
let a = null, b = 0, c = 1;
a ??= 'A'; b ||= 'B'; c &&= 'C';
log('assign1', a, b, c);
const p = { x: null, y: 1, deep: { k: 0 } };
p.x ??= 'X'; p.y ||= 'unused'; p.deep.k ||= 'K'; p['z'] ??= 'Z'; p.deep['w'] &&= 'nope';
log('assign2', p);
let n = 0; const holder = { get v() { n++; return holder._v; }, set v(x) { n += 10; holder._v = x; }, _v: null };
holder.v ??= 'set'; holder.v ??= 'again';
log('assign3', holder._v, n);
const getObj = () => { calls++; return p; }; calls = 0;
getObj().x ||= 'nope'; getObj().q ??= 'Q'; getObj()[side('r')] ||= 'R';
log('assign4', p.q, p.r, calls);

// **
log('exp', 2 ** 3, 2 ** 3 ** 2, (-2) ** 2, 2 ** -1, 2 ** 0.5 === Math.SQRT2);
let e = 3; e **= 2; const q = { v: 2, arr: [2] }; q.v **= 3; q.arr[0] **= 4; let idx = 0; q.arr[idx++] **= 2;
log('exp2', e, q.v, q.arr, idx);
log('exp3', (side(2)) ** (side(3)), calls);

// numeric separators, optional catch
log('sep', 1_000_000, 0xFF_FF, 1_0.5_0, 0b1010_1010, 1e1_0);
try { throw new Error('boom'); } catch { log('catch', 'no binding'); }
log('trailing', Math.max(1, 2,), ((a, b,) => a + b)(1, 2,), (function (x,) { return x; })(7,));

log('esm', answer, typeof helper?.call, answer ?? 0);
