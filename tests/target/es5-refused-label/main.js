outer: for (let p = 0; p < 2; p++) {
  const keep = () => p;
  for (let q = 0; q < 2; q++) { const f = () => q; if (f() === 1) continue outer; console.log(keep()); }
}
