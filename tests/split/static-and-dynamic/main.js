import { value, bump } from './both.js';
console.log('main', value);
bump();
const m = await import('./both.js');
console.log('dynamic sees', m.value, m.default);
const n = await import('./only-dynamic.js');
console.log(n.name, n.useBoth());
