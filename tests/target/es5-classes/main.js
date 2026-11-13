class A {
  constructor(x) { this.x = x; }
  get double() { return this.x * 2; }
  set double(v) { this.x = v / 2; }
  m() { return "A.m" + this.x; }
  static s() { return "A.s"; }
}
class B extends A {
  y = 5;
  static z = 7;
  static { B.w = B.z + 1; }
  constructor(x) { super(x + 1); this.t = () => this.x + this.y; }
  m() { return "B>" + super.m(); }
  static s() { return "B>" + super.s(); }
  ["comp" + 1]() { return 1; }
}
class C extends B {}
const b = new B(1), c = new C(5);
console.log(b.m(), B.s(), b.double, b.t(), B.z, B.w, b.comp1());
b.double = 100; console.log(b.x, c.m(), c instanceof A, Object.keys(b), typeof A);
try { A(); } catch (e) { console.log(e instanceof TypeError); }
const D = class { constructor() { this.k = new.target === D; } };
console.log(new D().k);
class E extends Error { constructor(m) { super(m); this.name = "E"; } }
try { throw new E("boom"); } catch (e) { console.log(e instanceof E, e instanceof Error, e.message, String(e)); }
class F { static make() { return new this(); } who() { return "F"; } }
console.log(F.make().who(), Object.keys(F.prototype), Object.getOwnPropertyNames(F.prototype));
class G extends A { m() { const f = () => super.m(); return f(); } }
console.log(new G(3).m());
class H extends Array { sum() { return this.reduce((a, b) => a + b, 0); } }
console.log(new H(1, 2, 3).sum(), H.from ? "from" : "nofrom");
