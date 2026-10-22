import { shared } from './shared.js';
console.log('start', shared);
const m = await import('./lazy.js');
console.log('after import', m.value, m.fn(), m.default);
const again = await import('./lazy.js');
console.log('same namespace', again === m);
const [a, b] = await Promise.all([import('./other.js'), import('./other.js')]);
console.log(a.name, a === b);
