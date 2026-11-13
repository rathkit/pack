// Forms the minifier writes shorter; each must still mean the same.
const kinds = [1, 'a', null, undefined, {}, [], () => 1, 2n, true, Symbol('s')];
const out = [];

for (const v of kinds) {
  out.push([
    typeof v === 'string', typeof v !== 'number', 'object' === typeof v, typeof v === typeof 'x',
    v === null || v === void 0, v !== null && v !== undefined, undefined === v || null === v,
  ].join(''));
}

function pick(n) {
  if (n < 0) return 'neg';
  else if (n === 0) return 'zero';
  else if (n < 10) return 'small';
  else return 'big';
}
function chain(n) {
  if (n < 0) {
    return -1;
  } else if (n > 0) {
    return 1;
  } else if (n === 0) return 0;
  return NaN;
}
out.push([-5, 0, 3, 50].map(pick).join('|'), [-1, 0, 1, NaN].map(chain).join('|'));

const table = { [0]: 'zero', [1]: 'one', [2.5]: 'frac', ['s']: 'str', [3 - 1]: 'two' };
out.push(JSON.stringify(table), Object.keys(table).join());

class Counter {
  #count = 0;
  #log = [];
  static #made = 0;
  static make() { Counter.#made++; return new Counter(); }
  static get made() { return Counter.#made; }
  #bump(by) { this.#count += by; this.#log.push(by); return this; }
  add(by) { return this.#bump(by); }
  get total() { return this.#count; }
  get seen() { return this.#log.join('+'); }
  static is(o) { return #count in o; }
  inner() {
    const self = this;
    return class { #count = 100; peek() { return self.#bump(0).total + this.#count; } };
  }
}
const c = Counter.make().add(2).add(3);
out.push(c.total, c.seen, Counter.made, Counter.is(c), Counter.is({}), new (c.inner())().peek());

function sum(a, b,) { return a + b; }
const lists = [1, 2, 3,];
const holes = [1, , ];
const obj = { a: 1, b: 2, };
out.push(sum(1, 2,), lists.length, holes.length, Object.keys(obj).length);
console.log(out.join('\n'));
