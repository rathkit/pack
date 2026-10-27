import { odd } from './cycle-y.js';
export function even(n) { return n === 0 ? true : odd(n - 1); }
export { odd };
