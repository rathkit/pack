// A package of re-exports that has no side effects: the entry reads one of them, a lazy module another. The other must not
// be loaded with the entry.
import { used } from './lib/index.js';
console.log(used);
import('./lazy.js').then((m) => console.log(m.later()));
