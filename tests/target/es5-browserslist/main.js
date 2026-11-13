const f = (a) => `v${a}`;
class K { m() { return f(1); } }
console.log(new K().m());
