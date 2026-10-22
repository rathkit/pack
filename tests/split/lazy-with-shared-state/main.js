import { store } from './store.js';
store.items.push('main');
const { addLazy } = await import('./lazy.js');
addLazy();
const { read } = await import('./other-lazy.js');
console.log(read(), store.items.join());
