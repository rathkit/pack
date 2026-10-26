import Anon, { Counter, Lazy } from './shared.js';
const show = (label, v) => console.log(label, JSON.stringify(v));
const c = new Counter();
show('counter', [c.inc(), c.inc(), c.tag, Counter.made, Counter.has(c), Counter.isCounter(c), Counter.isCounter({})]);
const a = new Anon();
show('default', [a.value, a.reveal(), Anon.kind, typeof Anon.name]);
const l = new Lazy();
show('lazy', [l.extra, l.show(), l instanceof Counter, Counter.made, Counter.isCounter(l)]);
export class Local {
  static #count = 0;
  id = ++Local.#count;
  static total() { return Local.#count; }
}
new Local(); new Local();
show('local', [new Local().id, Local.total()]);
export { Local as Renamed };
