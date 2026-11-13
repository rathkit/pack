const log = [];
const iter = { [Symbol.iterator]() { let i = 0; return { next() { return { value: i++, done: i > 5 }; }, return() { log.push("return"); return {}; } }; } };
for (const v of iter) { if (v === 1) break; }
lbl: for (const a of iter) { for (const b of iter) { if (b === 0) continue lbl; } }
function f() { for (const v of iter) { return v; } }
f();
try { for (const v of iter) { throw new Error("x"); } } catch (e) { log.push("caught"); }
for (const v of iter) { }
console.log(log.join());
const bad = { [Symbol.iterator]() { return { next() { throw new Error("n"); }, return() { log.push("BAD"); } }; } };
try { for (const v of bad) { } } catch (e) { console.log(e.message, log.includes("BAD")); }
