const sleep = (v) => new Promise((r) => setTimeout(() => r(v), 1));
async function f(a) { const x = await sleep(a); const y = (await sleep(x)) + 1; return x + y; }
async function g() { try { await Promise.reject(new Error("no")); } catch (e) { return "caught " + e.message; } finally { console.log("fin"); } }
const arrow = async (n) => { let s = 0; for (let i = 0; i < n; i++) s += await sleep(i); return s; };
const expr = async (v) => await sleep(v) * 2;
class C { constructor() { this.v = 3; } async m() { return (await sleep(this.v)) + 1; } static async s() { return "s"; } }
const o = { async m() { return await sleep("o"); } };
async function* ag() { yield 1; const v = await sleep(2); yield v; }
async function main() {
  console.log(await f(1), await g(), await arrow(4), await expr(5), await new C().m(), await C.s(), await o.m());
  const out = [];
  for await (const v of [sleep(1), 2]) out.push(v);
  console.log(out);
  const ar = [];
  for await (const v of (async function* () { yield "a"; yield "b"; })()) ar.push(v);
  console.log(ar);
  console.log(await Promise.all([1, 2].map(async (x) => (await sleep(x)) * 10)));
  try { await (async () => { throw new TypeError("t"); })(); } catch (e) { console.log(e.name); }
  function inner() { return arguments.length; }
  const withArgs = async function () { return arguments.length + (await sleep(0)); };
  console.log(inner(1, 2), await withArgs(1, 2, 3));
}
main().then(() => console.log("end"));
