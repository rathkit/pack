import { util } from './vendor/util.js';
console.log(util());
import('./lazy.js').then((m) => console.log(m.default));
