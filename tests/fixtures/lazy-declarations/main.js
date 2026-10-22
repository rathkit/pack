const m = await import('./lazy.js');
console.log(m.a, m.b, m.c, m.d, m.F.name, new m.G().hi(), m.h(), m.def.k, m.sum, m.obj.x);
const n = await import('./lazy2.js');
console.log(n.a, typeof n.b, n.b());
