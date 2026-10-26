const show = (label, v) => console.log(label, JSON.stringify(v, (k, x) => (typeof x === 'symbol' ? x.toString() : typeof x === 'function' ? 'fn:' + x.name : x)));
const kind = (fn) => { try { fn(); return 'no error'; } catch (e) { return e.constructor.name; } };
const log = [];
const tap = (label, v) => { log.push(label); return v; };

// public fields: keys of every kind, values that see earlier fields and `this`
const sym = Symbol('field');
class Fields {
  plain = 1;
  'quoted key' = 2;
  42 = 3;
  [tap('computed', 'comp' + 'uted')] = 4;
  [sym] = 5;
  static = 6;
  get = 7;
  set = 8;
  async = 9;
  later = this.plain + 10;
  arrow = () => this.plain;
  nothing;
  static count = 0;
  static label = 'Fields:' + typeof Fields.name;
}
const f = new Fields();
show('fields1', [f, f.arrow(), Object.keys(f), f[sym], 'nothing' in f, Fields.label, Fields.count]);
show('fields-order', log);

// a field defines its property, it does not call a setter of the prototype
class WithSetter { set x(v) { this.calls = (this.calls || 0) + 1; } }
class Defines extends WithSetter { x = 'own'; }
const d = new Defines();
show('define-semantics', [d.x, d.calls, Object.getOwnPropertyDescriptor(d, 'x').writable]);

// inheritance: order of initializers, synthesized constructors, `super()` in several places
const order = [];
class Base { b1 = order.push('base field'); constructor() { order.push('base ctor'); } }
class Mid extends Base { m1 = order.push('mid field'); }
class Leaf extends Mid { l1 = order.push('leaf field'); constructor(flag) { if (flag) { super(); } else { super(); } order.push('leaf ctor'); } }
new Leaf(true); new Leaf(false);
show('inherit-order', order);
class Arrow extends Base { a = 'a'; constructor() { const call = () => super(); call(); this.after = this.a; } }
show('super-in-arrow', new Arrow());
class Args extends Array { tag = 'tag'; }
const args = new Args(3);
show('extends-array', [args.length, args.tag, args instanceof Args]);
class Err extends Error { code = 42; }
show('extends-error', [new Err('m').message, new Err('m').code, new Err() instanceof Error]);
class Returns extends Base { r = 1; constructor() { super(); } }
show('derived', new Returns());

// private fields, methods, accessors, statics, brand checks
class Priv {
  #a = 1;
  #b;
  static #count = 0;
  static #helper() { return 'static helper'; }
  #method(x) { return this.#a + x; }
  get #acc() { return this.#a * 100; }
  set #acc(v) { this.#a = v; }
  get #readonly() { return 'ro'; }
  constructor() { Priv.#count++; }
  run() {
    this.#a++; ++this.#a; this.#a += 10; this.#a **= 2; this.#b ??= 'b'; this.#b ||= 'x'; this.#b &&= this.#b + '!';
    this.#acc = this.#acc + 1;
    const out = [this.#a, this.#b, this.#method(1), this.#acc, this.#readonly, Priv.#helper(), Priv.#count];
    [this.#a] = [7]; ({ x: this.#b } = { x: 'destructured' });
    for (this.#a of [1, 2, 3]);
    return [...out, this.#a, this.#b];
  }
  static has(o) { return #a in o; }
  static hasMethod(o) { return #method in o; }
  static read(o) { return o.#a; }
  static chained(o) { return o?.#a + (o?.#b ?? 'none'); }
  callThroughChain() { return this?.#method?.(5); }
  static count() { return Priv.#count; }
  equal(other) { return this.#a === other.#a; }
}
const p = new Priv();
show('private', p.run());
show('private-in', [Priv.has(p), Priv.has({}), Priv.hasMethod(p), Priv.hasMethod({})]);
show('brand-errors', [kind(() => Priv.read({})), kind(() => Priv.read(null)), kind(() => p.equal({})), kind(() => Priv.has(1))]);
show('chain-private', [Priv.chained(p), Priv.chained(null), Priv.chained(undefined), p.callThroughChain(), Priv.count()]);
show('private-json', [JSON.stringify(p), Object.keys(p), Object.getOwnPropertyNames(p)]);

// private names of nested classes: hiding and reaching outward
class Outer {
  #x = 'outer';
  #shadow = 'o-shadow';
  makeInner() {
    const outer = this;
    return new (class Inner {
      #y = 'inner';
      #shadow = 'i-shadow';
      both() { return [this.#y, outer.#x, this.#shadow]; }
      static peek(o) { return o.#shadow; }
    })();
  }
  static peekOuter(o) { return o.#x; }
  static peekShadow(o) { return o.#shadow; }
}
const inner = new Outer().makeInner();
show('nested-private', [inner.both(), Outer.peekOuter(new Outer()), kind(() => Outer.peekOuter(inner)), inner.constructor.peek(inner), Outer.peekShadow(new Outer()), kind(() => Outer.peekShadow(inner))]);

// accessors and private methods with this and arguments
class Methods {
  #sum(...n) { return n.reduce((a, b) => a + b, 0) + (this.base || 0); }
  #gen = function* () { yield 1; yield 2; };
  base = 100;
  total() { return [this.#sum(1, 2, 3), this.#sum(), [...this.#gen()]]; }
  static #s = 5;
  static get s() { return Methods.#s; }
  static set s(v) { Methods.#s = v; }
}
const mm = new Methods();
Methods.s = 9;
show('methods', [mm.total(), Methods.s]);

// static blocks and static fields run in order, `this` is the class
class Statics {
  static a = (log.push('static a'), 1);
  static { log.push('block 1 sees ' + this.a); this.fromBlock = 'set in block'; var blockLocal = 'local'; log.push(blockLocal); }
  static b = (log.push('static b'), this.a + 1);
  static { log.push('block 2 sees ' + this.b); }
  static self = this;
  static name2 = this.name === Statics.name;
  static nested = () => this.a;
  static fn = function () { return this === Statics; };
}
show('statics', [Statics.a, Statics.b, Statics.fromBlock, Statics.self === Statics, Statics.name2, Statics.nested(), Statics.fn(), log.slice(-6)]);

// class expressions
const Anonymous = class { static s = 'anon'; inst = 'i'; };
const Named = class Inner { static s = typeof Inner.name + '!'; static self = Inner; };
show('expressions', [Anonymous.s, new Anonymous().inst, Anonymous.name, Named.s, Named.self === Named, typeof Named.name]);
const make = () => class { static made = 'made'; field = 'f'; };
const Made = make();
show('expr-return', [Made.made, new Made().field, typeof Made]);
const registry = { Cls: class { value = 'in-object'; } };
show('expr-property', [new registry.Cls().value, typeof registry.Cls.name]);
show('expr-arg', [[class { x = 1; }].map((C) => new C().x)]);
function scoped() { class Local { local = 'local'; static s = 'ls'; } return [new Local().local, Local.s]; }
show('scoped', scoped());
{ class Block { b = 'block'; } show('block', new Block()); }
show('class-field-class', new (class { Inner = class { deep = 'deep'; }; })().Inner.name);
show('field-class', [new (class Holder { C = class { x = 1 }; }) ().C.name]);
