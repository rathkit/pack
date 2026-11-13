for (const v of [1, 2]) console.log(v);
for (const [i, w] of [[1, 2], [3, 4]]) console.log(i + w);
for (const c of "a𝒳") console.log(c.length);
for (const [k, v] of new Map([["a", 1], ["b", 2]])) console.log(k, v);
for (const s of new Set([5, 6])) { if (s === 5) continue; console.log(s); }
let last; for (last of [7, 8]); console.log(last);
const target = {}; for (target.v of [9]); console.log(target.v);
for (const { a, b = 2 } of [{ a: 1 }]) console.log(a, b);
function inner() { for (const x of [1, 2, 3]) { if (x === 2) return x; } }
console.log(inner());
outer: for (const x of [1, 2]) { for (const y of [1, 2]) { if (y === 2) continue outer; console.log(x, y); } }
function args() { for (const a of arguments) console.log(a); }
args("x", "y");
