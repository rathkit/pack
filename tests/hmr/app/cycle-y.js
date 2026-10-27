import { even } from './cycle-x.js';
export function odd(n) { return n === 0 ? false : even(n - 1); }
