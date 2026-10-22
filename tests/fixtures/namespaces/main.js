import * as lib from './lib.js';
import * as star from './star.js';
console.log(lib.a(), lib.b, lib.missing);
console.log(Object.keys(lib), Object.keys(star));
const copy = { ...lib };
console.log(typeof copy.a, copy.b);
function take(ns) { return ns.a() + ns.b; }
console.log(take(lib), star.deep);
console.log(lib[Symbol.toStringTag], Object.prototype.toString.call(star));
