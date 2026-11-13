function* a() { var x = yield 1; var y = x ? yield "t" : yield "f"; return [x, y]; }
const i1 = a(); console.log(i1.next(), i1.next(1), i1.next("T"), i1.next());
function* b(arg) { const l = (yield 1) || (yield 2); const m = (yield 3) && (yield 4); const n = (yield 5) ?? (yield 6); return [l, m, n, arguments.length, arg]; }
const i2 = b("A", "B"); console.log(i2.next(), i2.next(0), i2.next(0), i2.next(1), i2.next(1), i2.next(null), i2.next(8));
function* c() { outer: for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) continue outer; if (i === 2) break outer; yield [i, j]; } } blk: { yield "in"; if (true) break blk; yield "never"; } yield "out"; }
console.log([...c()]);
function* e() { try { try { yield 1; throw "inner"; } finally { yield "f1"; } } catch (x) { yield "c:" + x; } try { return "r"; } finally { yield "f2"; } }
console.log([...e()]);
function* f() { yield this.k; const g = () => this.k; yield g(); function helper() { return "h"; } yield helper(); var z = 1; { var w = 2; } yield z + w; }
console.log([...f.call({ k: "K" })]);
function* h() { const { p, q = 5 } = { p: 1 }; yield "need q"; const [r, s] = [yield "r", 2]; return [p, q, r, s, yield "x"]; }
const i5 = h(); console.log(i5.next(), i5.next("Q"), i5.next("R"));
function* t() { yield `a${yield 1}b`; const o = { k: yield 2, l: 3 }; yield o; yield [yield 3, 4].length; yield (yield 4, yield 5); }
const i6 = t(); const out = []; let r = i6.next(); let n = 0; while (!r.done) { out.push(r.value); r = i6.next("v" + n++); } console.log(out);
function* inf() { let n = 0; while (true) yield n++; }
for (const v of inf()) { if (v > 2) break; console.log(v); }
function* th() { try { yield 1; } catch (e) { yield "got " + e; } }
const i7 = th(); i7.next(); console.log(i7.throw("boom"), i7.next());
console.log(typeof a().next, a()[Symbol.iterator] !== undefined);
