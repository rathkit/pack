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
// Guards at the end of a body, loops that skip, switches that fall into default, else-if chains of returns.
function guarded(items, log) {
  for (const x of items) {
    if (!x) continue;
    log.push('a' + x);
    log.push('b' + x);
  }
  for (const x of items) {
    if (x > 1) continue;
    log.push('c' + x);
  }
  let i = 0;
  while (i < 3) {
    i++;
    if (i === 2) continue;
    log.push('w' + i);
  }
  return log.join('');
}
function early(a, log) {
  if (a) return;
  log.push('x');
  log.push('y');
}
function fall(v) {
  switch (v) {
    case 'a':
    case 'b':
    default:
      return 'd';
    case 'c':
      return 'c';
  }
}
function fall2(v) {
  let r = '';
  switch (v) {
    case 1: r += '1';
    case 2:
    default: r += 'x';
    case 3: r += '3';
  }
  return r;
}
function tri(n) {
  if (n < 0) return -1;
  else if (n > 0) return 1;
  return 0;
}
const l1 = [], l2 = [];
early(0, l1); early(1, l2);
out.push(guarded([0, 1, 2], []), l1.join(''), l2.join(''), ['a', 'b', 'c', 'z'].map(fall).join(), [1, 2, 3, 4].map(fall2).join(), [-3, 0, 3].map(tri).join());
console.log(out.join('\n'));
