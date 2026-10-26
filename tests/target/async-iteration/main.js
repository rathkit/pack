const out = [];
const log = (label, v) => out.push(label + ' ' + JSON.stringify(v));
const tick = (v) => new Promise((r) => setTimeout(() => r(v), 1));
const collect = async (iterable) => { const r = []; for await (const x of iterable) r.push(x); return r; };

async function* count(n) { for (let i = 0; i < n; i++) { await tick(); yield i; } return 'done'; }
async function* mixed() { const a = yield 1; const b = yield await tick(a + 1); yield [a, b]; }
async function* delegating() { yield 'start'; const r = yield* count(3); yield 'r=' + r; yield* [10, 20]; yield* (async function* () { yield 'inner'; })(); }
async function* throwing() { try { yield 1; yield 2; } finally { out.push('cleanup'); } }
async function* rejecting() { yield 1; throw new Error('gen-fail'); }
async function* yieldsPromise() { yield tick('p1'); yield Promise.resolve('p2'); }
const syncIterable = { *[Symbol.iterator]() { yield tick('s1'); yield 's2'; } };

class K {
  base = 'k';
  async *items() { yield this.base + 1; yield this.base + 2; }
  static async *stat() { yield 'static'; }
  async *['comp' + 'uted']() { yield 'computed'; }
}
const o = { async *gen() { yield 'obj'; }, async *[`k${1}`]() { yield 'k1'; } };

class Store { set(k, v) { this.data = this.data || {}; this.data[k] = v; return v; } }
class Cell {
  get value() { return this._v === undefined ? 10 : this._v; }
  set value(v) { this._v = v; }
}
class Counter extends Cell {
  async bump() { await tick(); super.value = 5; super.value += 3; ++super.value; return [this.value, super.value--, this.value, (super['value'] = 'k'), this.value]; }
  plain() { const later = async () => { await tick(); super.value = 'plain'; return super.value; }; return later(); }
  nonAsyncOuter() { return [1, 2].map(async (n) => (await tick(n)) + this.value + super.value); }
  ctorLike() { const f = async () => super.constructor === Cell; return f(); }
}
class Slots { get a() { return this._a === undefined ? 1 : this._a; } set a(v) { this._a = v; } get b() { return this._b; } set b(v) { this._b = v; } get n() { return this._n; } set n(v) { this._n = v; } }
const keys = [];
const key = (k) => { keys.push(k); return k; };
class Wide extends Slots {
  async logical() {
    const r = [];
    super.b ||= 'first'; r.push(this.b);
    super.b ||= 'second'; r.push(this.b);
    super.a &&= await tick(7); r.push(this.a);
    super.n ??= await tick('set'); r.push(this.n);
    super.n ??= 'again'; r.push(this.n);
    r.push(super['a'] **= await tick(2), (super[key('a')] += await tick(1)), (super[key('n')] ||= 'x'), (super[key('a')]++), (++super[key('a')]), keys);
    super[key('a')] -= 1; super[key('a')] &&= 100; r.push(this.a);
    return r;
  }
  async both() { const f = async () => { super.b = await tick('from-arrow'); return super.b; }; return [await f(), await f()]; }
}
class WithField extends Slots {
  base = 'wf';
  grab = async () => { await tick(); return super.a + this.base; };
  static make() { return new WithField(); }
}
class Base { greet(x) { return 'base:' + x; } get prop() { return 'bp'; } }
class Derived extends Base {
  async greet(x) { await tick(); return 'derived>' + super.greet(x) + ':' + super.prop + ':' + super['greet']('c'); }
  async arrow() { const f = async (y) => super.greet(y + (await tick(1))); return f('a'); }
  async *gen() { yield super.greet('g'); }
}

(async () => {
  log('count', await collect(count(3)));
  log('sync', await collect(syncIterable));
  log('promise-yield', await collect(yieldsPromise()));
  log('class', [await collect(new K().items()), await collect(K.stat()), await collect(new K().computed()), await collect(o.gen()), await collect(o.k1())]);

  const g = mixed();
  log('next-values', [(await g.next('ignored')).value, (await g.next(5)).value, (await g.next('B')).value, (await g.next()).done]);
  log('delegating', await collect(delegating()));

  // break closes the iterator
  const seen = [];
  for await (const x of throwing()) { seen.push(x); break; }
  log('break', [seen, out.includes('cleanup')]);
  // continue, labels of plain loops, nesting
  const nested = [];
  for await (const a of count(3)) { if (a === 1) continue; for await (const b of count(2)) nested.push(a + ':' + b); }
  log('nested', nested);
  // errors
  try { for await (const x of rejecting()) seen.push(x); } catch (e) { log('error', e.message); }
  try { for await (const x of [Promise.reject(new Error('rejected-item'))]) {} } catch (e) { log('item-error', e.message); }
  // assignment targets and destructuring
  let t, u; const holder = {};
  for await (t of count(2)) u = t;
  for await (holder.x of ['h']) {}
  for await ([t, u] of [[1, 2]]) {}
  for await ({ t } of [{ t: 'obj-pattern' }]) {}
  log('targets', [t, u, holder.x]);
  // closures capture per-iteration bindings
  const fns = []; for await (const v of count(3)) fns.push(() => v);
  log('closures', fns.map((f) => f()));
  // concurrency: two loops at once must not share state
  const [one, two] = await Promise.all([collect(count(3)), collect(count(2))]);
  log('concurrent', [one, two]);
  // super
  const d = new Derived();
  log('super', [await d.greet('x'), await d.arrow(), await collect(d.gen())]);
  // return / throw on async generator objects
  const r = count(5); await r.next();
  log('return', [await r.return('early'), await r.next()]);
  const th = throwing(); await th.next();
  try { await th.throw(new Error('thrown-in')); } catch (e) { log('throw', e.message); }
  log('protocol', [typeof count()[Symbol.asyncIterator], count()[Symbol.asyncIterator]() !== undefined, Object.prototype.toString.call(count())]);
  // labels, break/continue to them, nested labels
  const labeled = [];
  outer: for await (const a of count(3)) {
    inner: for await (const b of count(3)) {
      if (b === 1) continue inner;
      if (a === 1) continue outer;
      if (a === 2) break outer;
      labeled.push(a + ':' + b);
    }
  }
  both: nested2: for await (const a of [1, 2, 3]) { if (a === 2) break both; labeled.push('n' + a); }
  log('labeled', labeled);
  const c = new Counter();
  log('super-write', await c.bump());
  log('super-in-arrow', [await c.plain(), await Promise.all(new Counter().nonAsyncOuter()), await c.ctorLike()]);
  log('super-compound', await new Wide().logical());
  log('super-both', await new Wide().both());
  log('super-field', await WithField.make().grab());
  console.log(out.join('\n'));
})();
