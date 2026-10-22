import Klass, * as ns from './klass.cjs';
import { make } from './make.js';
const a = new Klass();
const b = new Klass;
const c = new ns.default();
console.log(a.hi(), b.hi(), c.hi(), new (make())().hi());
const fallback = (this && this.nothing) || 'top-level-this-is-undefined';
console.log(fallback, typeof this, [this].length);
console.log(`${Klass.name}`, Klass.kind?.length);
