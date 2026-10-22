import { b } from './b.js';
export function a() { return 'a>' + b(); }
export const early = 'early-a';
console.log('eval a', typeof b);
