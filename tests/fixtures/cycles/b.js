import { a, early } from './a.js';
export function b() { return 'b'; }
console.log('eval b', typeof a, typeof early === 'undefined' ? 'tdz-safe' : early);
