import { value as a, run as runA } from './a.js';
import { value as b, run as runB } from './b.js';
import { value as c } from './c.js';
const value = 'main';
function run() { return 'main-run'; }
console.log(a, b, c, value, run(), runA(), runB());
