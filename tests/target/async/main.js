const out = [];
const log = (label, v) => out.push(label + ' ' + JSON.stringify(v));
const tick = (v) => new Promise((r) => setTimeout(() => r(v), 1));

async function decl(a, b = 2) { return a + (await b); }
const expr = async function named(x) { return (await x) * 2; };
const arrow = async (x) => (await x) + 1;
const arrowBlock = async x => { const y = await x; return y + 10; };
const arrowObject = async () => ({ ok: await tick(true) });
const nested = async (x) => { const inner = async (y) => (await y) + (await x); return await inner(await tick(5)); };

class K {
  base = 100;
  async m(x) { return this.base + (await x); }
  static async s() { return this === K; }
  async ['comp' + 'uted']() { return 'c' + (await 1); }
}
const o = { v: 7, async f() { return this.v + (await 1); }, async [`k${1}`]() { return 'k1'; }, g: async function () { return this.v; }, h: async () => typeof this };
async function usesArguments() { await 0; return [...arguments]; }
function outer() { return (async () => { await 0; return arguments[0]; })(); }
async function precedence() { const a = Promise.resolve(2); return [(await a) + (await a), (await a) ** 3, -(await a), !(await a), (await a) ? 'y' : 'n', typeof (await a)]; }
async function control() {
  const seen = [];
  for (const x of [1, 2, 3]) seen.push(await x * 2);
  let i = 0; while (i < 3) { seen.push(await tick(i)); i++; }
  try { await Promise.reject(new Error('boom')); } catch (e) { seen.push(e.message); } finally { seen.push('fin'); }
  try { await (async () => { throw new RangeError('inner'); })(); } catch (e) { seen.push(e.name); }
  return seen;
}
async function syncThrow() { throw new TypeError('sync'); }
async function thenable() { return await { then(res) { res('thenable'); } }; }
async function order() {
  const trace = [];
  const p = (async () => { trace.push('a1'); await null; trace.push('a2'); await null; trace.push('a3'); })();
  trace.push('b1');
  Promise.resolve().then(() => trace.push('p1')).then(() => trace.push('p2'));
  trace.push('b2');
  await p;
  return trace;
}
const isPromise = (f) => f() instanceof Promise;

(async () => {
  log('basic', [await decl(1), await decl(1, 5), await expr(4), await arrow(1), await arrowBlock(1), await arrowObject(), await nested(3)]);
  const k = new K();
  log('methods', [await k.m(1), await K.s(), await k.computed(), await o.f(), await o.k1(), await o.g(), await o.h()]);
  log('args', [await usesArguments(1, 2, 3), await outer('first')]);
  log('precedence', await precedence());
  log('control', await control());
  log('errors', [await syncThrow().catch((e) => e.constructor.name), await thenable()]);
  log('order', await order());
  log('kinds', [isPromise(decl), decl.length, typeof decl.name, typeof expr.name, typeof arrow.name, K.prototype.m.name, typeof decl]);
  console.log(out.join('\n'));
})();

class P {
  #n = 1;
  async #step(x) { return this.#n + (await x); }
  static async #make() { return new P(); }
  async run() { return [await this.#step(2), (await P.#make()) instanceof P]; }
  static async go() { return (await new P().run()).concat(await P.#make().then((p) => p.#n)); }
}
P.go().then((v) => console.log('private-async', JSON.stringify(v)));
