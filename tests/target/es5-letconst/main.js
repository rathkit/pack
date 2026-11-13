let a = 1;
const b = [1, 2, 3];
{ let a = 5; console.log(a); }
console.log(a);
function f() {
  let x = 1;
  { let x = 2; console.log(x); }
  console.log(x);
  for (let i = 0; i < 2; i++) { let y; console.log(i, y); y = i; }
  { const x2 = 3; { const x2 = 4; console.log(x2); } console.log(x2); }
  return x;
}
console.log(f());
var outside = "outer";
function shadow() { { let outside = "inner"; } return outside; }
console.log(shadow());
switch (1) { case 1: { let z = 1; console.log(z); } }
const fns = [];
for (let i = 0; i < 3; i++) { fns.push(() => i); }
console.log(fns.map((g) => g()));
const gs = [];
for (const x of ["a", "b", "c"]) { let y = x + "!"; gs.push(() => x + y); if (x === "b") continue; }
console.log(gs.map((g) => g()));
function find(list) {
  for (const item of list) {
    const get = () => item * 2;
    if (get() > 4) return get();
    if (item < 0) break;
  }
  return -1;
}
console.log(find([1, 2, 3, 4]), find([-1, 5]), find([1]));
const hs = [];
for (const k in { p: 1, q: 2 }) hs.push(() => k);
console.log(hs.map((g) => g()));
let n = 0; const ws = [];
while (n < 3) { const m = n; ws.push(() => m); n++; }
console.log(ws.map((g) => g()));
outer: for (let p = 0; p < 3; p++) { const keep = () => p; for (let q = 0; q < 3; q++) { if (q === 1) continue outer; if (p === 2) break outer; console.log(keep() + q); } }
