// A package of re-exports that has no side effects: the entry reads one of them, a lazy module another. The other must not
// be loaded with the entry.
import { used, lazyOnly } from './lib/index.js'; // lazyOnly is imported and not used: it must not bring the lazy code in
console.log(used);
import('./lazy.js').then((m) => console.log(m.later()));
