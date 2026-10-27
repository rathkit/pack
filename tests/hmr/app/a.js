export let n = 0;
export function inc() { n++; }
export const answer = 42;
export class Counter { constructor(s) { this.s = s; } next() { return ++this.s; } }
export default function () { return 'anonymous default'; }
export { kind, helper as alias } from './b.js';
export { helper } from './b.js';
export * from './deep.js';
export * as deepNs from './deep.js';
export function whoami() { return this; }
