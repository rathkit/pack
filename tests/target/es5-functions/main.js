function outer() {
  const arrow = () => [this && this.v, arguments.length, arguments[0]];
  const nested = () => () => this.v;
  return [arrow(), nested()()];
}
console.log(JSON.stringify(outer.call({ v: 3 }, "x", "y")));
const obj = { v: 1, get() { return (() => this.v)(); }, n: (a, b = a + 1) => a + b };
console.log(obj.get(), obj.n(1));
function dflt(a, { b = 2, c } = {}, [d, e] = [4, 5], ...rest) { return [a, b, c, d, e, rest.length].join(); }
console.log(dflt(1), dflt(1, { c: 3 }, [7], 9, 9));
console.log(((...z) => z.length)(1, 2, 3), (() => 1)(), (x => x * 2)(4));
const fnExpr = function named(n) { return n ? named(n - 1) + 1 : 0; };
console.log(fnExpr(3));
function undef(a = 1) { return a; }
console.log(undef(), undef(undefined), undef(null), undef(0));
const mk = () => ({ a: 1 });
console.log(mk().a);
if (true) { function inBlock() { return "blk"; } console.log(inBlock()); }
function newTarget() { return new.target === undefined ? "call" : "new"; }
console.log(newTarget(), typeof new newTarget());
