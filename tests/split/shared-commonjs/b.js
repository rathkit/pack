import cjs, { named } from './lib.cjs';
import { n } from './esm.js';
console.log('b', cjs.id(), named, n);
const d = await import('./lib.cjs');
console.log(d.default === cjs);
