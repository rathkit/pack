// Shorter spellings of the same program: every line prints, so a wrong rewrite shows.
const log = (...a) => console.log(...a);

// true, false, undefined
log(true.toString(), false + 1, typeof undefined, [undefined][0], undefined ?? 5);
function f1(undefined) { return undefined; }
log(f1(3));
const o = { undefined, true: 1, false: 2, "quoted-key": 3, "ok": 4, 5: 6, "7": 8, 'single': 9 };
log(o);
log(`x${true}y${undefined}`, void 0 === undefined, null ?? undefined);
class Base { static v = 1 }
class Derived extends (true ? Base : Object) {}
log(Derived.v);

// numbers
log(1000, 0.5, 0x10, 1e21, 1.5e-7, 0.1 + 0.2, 1..toString(), 1.0.toFixed(1), 5..constructor.name, 1_000_000, 0b101, 0o17, 100, 120000, 0.00015);
log({ 1000: "a", 0.5: "b", 0x10: "c" });

// property access
const p = { abc: 1, "x-y": 2, if: 3 };
log(p["abc"], p["x-y"], p?.["abc"], p["if"], 1["toFixed"](1), "str"["length"]);

// new
function K() { this.v = 1; }
K.prototype.m = function () { return this.v; };
log(new K().m(), new K().v, (new K).v, new K, new Date(0).getTime(), new (class { x = 2 })().x);

// braces and the dangling else
function g(a, b) { if (a) { if (b) return 1; } else return 2; return 3 }
log(g(1, 1), g(1, 0), g(0, 0));
function h(a, b) { if (a) { for (;;) { if (b) return "b"; break; } } else { return "e"; } return "end"; }
log(h(1, 1), h(1, 0), h(0, 0));
function w(a, b) { if (a) { while (b) { if (b > 1) { b = 0 } } } else { b = 5 } return b }
log(w(1, 3), w(0, 0));
function s(a) { if (a) { x = 1 } else { x = 2 } var x; return x }
log(s(1), s(0));
let n = 0;
for (let i = 0; i < 3; i++) { n += i }
log(n);
do { n++ } while (n < 10);
log(n);
outer: for (const i of [1, 2, 3]) { if (i === 2) { continue outer } log(i) }
if (n) { log("one") } else { log("none") }
if (!n) { log("none") } else if (n > 5) { log("big") } else { log("small") }
switch (n) { case 10: { log("ten"); break } default: { log("other") } }
try { throw new Error("e") } catch (e) { log(e.message) } finally { log("done") }

// declarations
var a1 = 1; var b1 = a1 + 1; let c1 = b1 * 2; let d1 = c1 + a1; const e1 = [a1, b1, c1, d1]; const f2 = e1.length;
log(e1, f2);
const fs = [];
for (let i = 0; i < 3; i++) { fs.push(() => i) }
log(fs.map((f) => f()));
var dup = 1; var dup = 2; log(dup);

// arrows
const ar1 = (x) => { return x + 1 };
const ar2 = () => { return { a: 1 } };
const ar3 = () => { return 1, 2 };
const ar4 = () => { return (1, 2) };
const ar5 = (x) => { return (y) => x * y };
const ar6 = async () => { return await 3 };
const ar7 = () => { return function () { return 7 } };
const ar8 = () => { return undefined };
log(ar1(1), ar2(), ar3(), ar4(), ar5(2)(4), ar7()(), ar8());
ar6().then(log);
for (var q = () => { return 1 in { 1: 1 } }; false;) { }
log(q());

// return
function r1() { return undefined }
function r2() { return void 0 }
function r3(x) { if (x) return undefined; return 1 }
log(r1(), r2(), r3(1), r3(0));

// branches that are decided
if (true) log("t1"); else log("t1-else");
if (false) log("f1"); else log("f1-else");
if (false) { var hoisted = 1; }
log(typeof hoisted);
if (true) { log("block") }
if (false) { log("never") }
const cond1 = true ? { a: 1 } : 2, cond2 = false ? 1 : [2], cond3 = true ? function () { return 7 } : 0;
log(cond1, cond2, cond3());
function dang(x) { if (x) if (true) return "a"; else return "b"; else return "c" }
log(dang(1), dang(0));
function dang2(x, y) { if (x) { if (false) log("never"); else if (y) return "y" } else return "no"; return "end" }
log(dang2(1, 1), dang2(1, 0), dang2(0, 0));

// line breaks that stand for semicolons
let z1 = 1
let z2 = 2
;[z1, z2] = [z2, z1]
log(z1, z2)
const arrow = () => { return 1 }
;(function () { log("iife") })()
let u = 1
if (true) u = 2
u++
log(u)

// a body whose only statement is an `if` on a constant that is not a literal: the braces stay
function emptied() { let n = 3, count = 0; while (n-- > 0) { if ((undefined ?? "")) { count++; } } return count }
log(emptied());

// found by the fuzzer: statements inside a dropped branch, and a line break that stood for a `;` before an `else`
function nestedDead(a) { if (a) if (false) { if (true) { log("never"); } else if (false) { log("never"); } } else { return "kept"; } return "after"; }
log(nestedDead(1), nestedDead(0));
function elseAfterGone(a) {
  let r = "";
  if (true) if (a) if (true) { const v = 1; } else r += "x"
  else {
    r += "y";
  }
  return r;
}
log(elseAfterGone(1), elseAfterGone(0));
