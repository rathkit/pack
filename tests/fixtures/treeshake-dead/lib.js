export function used() { return helper() + K; }
function helper() { return 'h'; }
const K = 'k';
export function dead1() { return dead2(); }
function dead2() { return new Missing(); }
export const dead3 = [1, 2, 3].map((x) => x);
const deadCall = Math.random();
export class DeadClass extends Missing2 {}
