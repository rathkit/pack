const show = (label, v) => console.log(label, JSON.stringify(v, (k, x) => (typeof x === 'symbol' ? x.toString() : x)));
const sym = Symbol('s');
const base = { a: 1, b: 2, [sym]: 'sym', get g() { return 'got'; } };
const order = [];
const tap = (x) => { order.push(x); return x; };

// spread
show('spread1', { ...base });
show('spread2', { x: 0, ...base, a: 'over', y: 9 });
show('spread3', { ...{ a: 1 }, ...{ a: 2, c: 3 }, ...null, ...undefined, ...'hi', ...[7, 8], ...5, ...true });
const withMethods = { ...{ p: 1 }, m() { return 'method'; }, get acc() { return 'accessor'; }, ['k' + 1]: 'computed' };
show('spread4', [withMethods.m(), withMethods.acc, withMethods.k1, Object.getPrototypeOf(withMethods) === Object.prototype]);
const inherited = { fromProto: 1 };
const withProto = { __proto__: inherited, own: 0, ...{ more: 2 } };
show('proto-first', [withProto.fromProto, Object.keys(withProto), Object.getPrototypeOf(withProto) === inherited]);
show('spread5', { [tap('k1')]: tap(1), ...{ [tap('k2')]: tap(2) }, [tap('k3')]: tap(3) });
show('order', order);
const copy = { ...base };
show('symbol', [copy[sym], Object.getOwnPropertyDescriptor(copy, 'g').get === undefined, copy.g]);
const target = { set a(v) { order.push('setter ' + v); }, a0: 0 };
show('no-setter', [{ ...target, ...{ a: 5 } }.a, order[order.length - 1]]);
class Cls { constructor() { this.own = 1; } method() {} }
show('class-instance', { ...new Cls() });
show('nested', { n: { ...base, z: { ...{ q: 1 } } }, ...{ ...{ w: 1 } } });
const fnSpread = (o) => ({ ...o, extra: true });
show('arrow', fnSpread({ k: 1 }));
show('shorthand', (() => { const a = 1, b = 2; return { a, ...{ c: 3 }, b }; })());

// rest in declarations
const { a, ...restA } = base;
show('rest1', [a, restA, Object.keys(restA), restA.g, restA[sym]]);
const { b: renamed, a: gone, ...restB } = { a: 1, b: 2, c: 3, d: 4 };
show('rest2', [renamed, gone, restB]);
const { 'quoted-key': q, c = 'dflt', ...restC } = { 'quoted-key': 'Q', z: 26 };
show('rest3', [q, c, restC]);
const { ...everything } = { m: 1 };
show('rest4', everything);
let { x: { y }, ...restD } = { x: { y: 'deep' }, e: 5 };
show('rest5', [y, restD]);
const { length, ...fromString } = 'ab';
show('rest7', [length, fromString]);
var v1 = 1, { k1, ...restE } = { k1: 'one', k2: 'two' }, v2 = 2;
show('rest8', [v1, k1, restE, v2]);

// rest in parameters
function plain({ a, ...rest }, second) { return [a, rest, second]; }
show('param1', plain({ a: 1, b: 2, c: 3 }, 's'));
const arrowExpr = ({ a, ...rest }) => ({ rest, a });
show('param2', arrowExpr({ a: 1, z: 26 }));
const arrowBlock = ({ a, ...rest }, ...more) => { return [a, rest, more]; };
show('param3', arrowBlock({ a: 1, z: 26 }, 'm1', 'm2'));
const withDefault = ({ a = 'A', ...rest } = { b: 'dflt' }) => [a, rest];
show('param4', [withDefault(), withDefault({ a: 'given', c: 3 })]);
const obj = { method({ a, ...rest }) { return [a, rest, this === obj]; }, async: 1 };
show('param5', obj.method({ a: 1, r: 2 }));
class WithRest { constructor({ id, ...opts }) { this.id = id; this.opts = opts; } static make({ n, ...rest }) { return new WithRest({ id: n, ...rest }); } }
show('param6', [new WithRest({ id: 7, flag: true }), WithRest.make({ n: 8, k: 'v' })]);
const countArgs = function ({ a, ...rest }, b, c) { return arguments.length + ':' + [a, b, c].join(',') + JSON.stringify(rest); };
show('param8', countArgs({ a: 1, z: 2 }, 'b'));

// rest in for-of
const out = [];
for (const { id, ...more } of [{ id: 1, v: 'a' }, { id: 2, v: 'b', w: 'c' }]) out.push([id, more]);
for (let { id, ...more } of [{ id: 3 }]) { id += 10; out.push([id, more]); }
for (var { id: vid, ...vmore } of [{ id: 4, k: 'k' }]) out.push([vid, vmore]);
show('forof', out);

// Object rest nested in other patterns, after computed keys, behind defaults, and in async arrows.
{
  const src = { a: 1, b: { c: 2, d: 3, e: 4 }, e: 5, f: 6, [Symbol.for('s')]: 7 };
  const { a, b: { c, ...bRest }, ...rest } = src;
  console.log('nested', a, c, bRest, rest, Object.getOwnPropertySymbols(rest).length);
  const { x: [first, { y, ...inArray }] = [1, { y: 2, w: 3, v: 4 }], ...others } = { z: 9 };
  console.log('in array', first, y, inArray, others);
  const [p, { q, ...afterQ }, ...tail] = [1, { q: 2, r: 3 }, 4, 5];
  console.log('array pattern', p, q, afterQ, tail);
  const key = 'k1';
  const log = [];
  const k = (name) => (log.push(name), name);
  const { [k('k1')]: v1 = 'dflt', [k('k2')]: v2, ...afterKeys } = { k1: undefined, k2: 'two', k3: 'three', 1: 'num' };
  console.log('computed', v1, v2, afterKeys, log);
  const { [1]: numeric, ...withoutOne } = { 1: 'one', 2: 'two' };
  console.log('numeric key', numeric, withoutOne);
  function f1({ a, ...r2 }, [x, { y2, ...z }] = [1, { y2: 2, w: 3 }]) { return [a, r2, x, y2, z]; }
  console.log('params', f1(src), f1({ a: 0 }, [5, { y2: 6, w: 7, v: 8 }]));
  const g = ({ a, b: { c, ...bb }, ...r3 }) => [a, c, bb, r3];
  console.log('arrow', g(src));
  for (const { b: { c, ...bb }, ...r4 } of [src, { b: { c: 0, z: 1 }, q: 1 }]) console.log('for of', c, bb, r4);
  const h = async ({ a, ...r5 }) => [a, r5];
  const h2 = async ({ b: { ...copy } }) => copy;
  Promise.all([h(src), h2(src)]).then((r) => console.log('async', r));
  let n1 = 0;
  const { w = (n1++, 'dflt'), ...r6 } = { u: 1 };
  console.log('default once', w, r6, n1);
}

// ... in assignments and in catch.
{
  let a, r, t;
  const o2 = {};
  const out = ({ a, ...r } = { a: 1, b: 2, c: 3 });
  console.log('assign', a, r, out.b);
  ({ a: o2.x, ...o2.rest } = { a: 5, z: 6 });
  console.log('assign to members', o2);
  [t, { ...r }] = [1, { q: 2 }];
  console.log('array assign', t, r);
  ({ k: { a, ...r } } = { k: { a: 7, s: 8 } });
  console.log('nested assign', a, r);
  const keyed = 'dyn';
  ({ [keyed]: a, ...r } = { dyn: 'D', other: 1, 3: 'x' });
  console.log('computed assign', a, r);
  try {
    throw { code: 1, msg: 'x', extra: true };
  } catch ({ code, ...info }) {
    console.log('catch', code, info);
  }
  try {
    throw { deep: { n: 1, m: 2 } };
  } catch ({ deep: { n, ...others } }) {
    console.log('catch nested', n, others);
  }
}
