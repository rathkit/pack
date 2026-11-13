class Counter {
  #count = 0;
  static #instances = 0;
  #bump() { return ++this.#count; }
  get #double() { return this.#count * 2; }
  set #double(v) { this.#count = v / 2; }
  static #make() { return new Counter(); }
  constructor() { Counter.#instances++; }
  inc() { this.#bump(); this.#count += 1; return this; }
  get value() { return this.#count; }
  dbl() { this.#double = 20; return this.#double; }
  static create() { return Counter.#make(); }
  static get instances() { return Counter.#instances; }
  static has(o) { return #count in o; }
}
const c = Counter.create().inc().inc();
console.log(c.value, c.dbl(), Counter.instances, Counter.has(c), Counter.has({}));
class Sub extends Counter { #own = "s"; own() { return this.#own; } }
console.log(new Sub().own(), Counter.instances);
try { Counter.prototype.inc.call({}); } catch (e) { console.log(e instanceof TypeError); }
