import { x, show } from './lib.js';
function f(show2) { var lib_x = 'local'; return [x, show2, lib_x, show()]; }
console.log(f('s'));
function g() { const x2 = 'g-local'; return () => x + x2; }
console.log(g()());
