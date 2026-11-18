import { i } from './vendor/i-entry.js';
import { s } from './vendor/s-shared.js';
console.log(i, s);
import('./lazy.js').then((m) => console.log(m.default));
