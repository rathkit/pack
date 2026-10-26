let count = 0;
export class Counter {
  #n = 0;
  static made = 0;
  static #registry = new Set();
  tag = 'c' + ++count;
  constructor() { Counter.made++; Counter.#registry.add(this); }
  inc() { return ++this.#n; }
  static has(o) { return Counter.#registry.has(o); }
  static isCounter(o) { return #n in o; }
}
export default class {
  value = 'default';
  #secret = 7;
  reveal() { return this.#secret; }
  static kind = 'anon-default';
}
export const Lazy = class extends Counter {
  extra = this.tag + '!';
  #hidden() { return 'hidden:' + this.extra; }
  show() { return this.#hidden(); }
};
