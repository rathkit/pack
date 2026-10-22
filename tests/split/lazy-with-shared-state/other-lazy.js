import { store } from './store.js';
import { addLazy } from './lazy.js';
addLazy();
export function read() { return store.items.length; }
