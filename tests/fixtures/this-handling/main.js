import { top, arrow, method } from './t.js';
console.log(this, top, arrow(), method.call('M'), typeof globalThis);
class K { static s = this === K; f = this.constructor === K; }
console.log(K.s, new K().f);
