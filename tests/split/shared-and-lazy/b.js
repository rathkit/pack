import { shared } from './shared.js';
import { helper } from './helper.js';
console.log('b', shared(), helper());
const lazy = await import('./lazy.js');
console.log('lazy', lazy.value, lazy.useShared());
