function* g() { for (let i = 0; i < 2; i++) { const f = () => i; yield f; } }
console.log([...g()].length);
