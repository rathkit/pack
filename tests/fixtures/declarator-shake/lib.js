// A declaration of several names where only some are used: the others, if free of effects, can go.
export var used = 1, unusedArray = [1, 2, 3], alsoUsed = used + 1, unusedFn = function () { return alsoUsed; }, tail = 'kept';
export const k1 = 'a', k2 = Symbol('b'), k3 = new Map([[1, 2]]), k4 = Object.keys, k5 = Array.prototype.indexOf;

var counter = 0, tick = () => ++counter, noisy = tick(), quiet = 5;
let first = 1, { x, y } = { x: 10, y: 20 }, last = first + x;
const pure1 = new Set([1, 2]), pure2 = new WeakMap(), local = [last];

export function report() {
  return [used, alsoUsed, counter, last, local.length, tail, quiet].join(',');
}
export function unusedHelper() {
  return unusedArray.length + y + pure1.size;
}
