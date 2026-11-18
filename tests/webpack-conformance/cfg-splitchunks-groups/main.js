import { greet } from './vendor/lib.js';
console.log(greet('main'));
import('./lazy.js').then((m) => console.log(m.default));
