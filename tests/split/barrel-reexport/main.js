// A file of re-exports is not in the output, but what it re-exports has to be.
import { greet, answer } from './barrel.js';
console.log(greet('split'), answer);
import('./lazy.js').then((m) => console.log(m.later()));
