function* count(n) { for (let i = 0; i < n; i++) { const x = yield i; if (x) console.log("got", x); } return "done"; }
const it = count(3);
console.log(it.next(), it.next("a"), it.next(), it.next(), it.next());
console.log([...count(4)]);
function* nested() { yield 1; yield* [2, 3]; const r = yield* count(2); yield r; }
console.log([...nested()]);
function* tc() { try { yield 1; throw new Error("x"); } catch (e) { yield "caught " + e.message; } finally { yield "fin"; } yield "end"; }
console.log([...tc()]);
function* ex() { try { yield 1; yield 2; } finally { console.log("cleanup"); } }
const e = ex(); e.next(); console.log(e.return(9), e.next());
const o = { *g() { yield this.v; }, v: 7 };
console.log([...o.g()]);
class K { *[Symbol.iterator]() { yield "k1"; yield "k2"; } static *s() { yield 5; } }
console.log([...new K()], [...K.s()]);
function* sw(x) { switch (x) { case 1: yield "one"; break; case 2: yield "two"; default: yield "dflt"; } }
console.log([...sw(1)], [...sw(2)], [...sw(3)]);
function* wh() { let i = 0; while (true) { if (i > 2) break; yield i++; } do { yield "d" + i; i--; } while (i > 1); }
console.log([...wh()]);
function* fo() { for (const k in { a: 1, b: 2 }) yield k; for (const v of [7, 8]) { if (v === 8) continue; yield v; } }
console.log([...fo()]);
function* ex2() { const a = (yield 1) + (yield 2); const b = [yield 3, yield 4]; return a + b.join(); }
const g2 = ex2(); console.log(g2.next(), g2.next(10), g2.next(20), g2.next("p"), g2.next("q"));
