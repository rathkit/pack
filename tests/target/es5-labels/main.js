outer: for (let p = 0; p < 3; p++) {
  const keep = () => p;
  mid: for (let q = 0; q < 3; q++) {
    const f = () => q;
    for (let r = 0; r < 3; r++) {
      const h = () => r;
      if (r === 1) continue mid;
      if (q === 1) continue outer;
      if (p === 2) break outer;
      console.log(keep(), f(), h());
    }
  }
}
function find() { a: for (const x of [1, 2, 3]) { for (const y of [1, 2]) { const g = () => x * y; if (g() === 4) return "found"; if (y === 2) continue a; } } return "none"; }
console.log(find());
