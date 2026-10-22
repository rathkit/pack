import { getV, v } from './a.js';
import { getV as getV2 } from './b.js';
console.log(getV(), getV2(), v, typeof hoisted, hoisted());
function hoisted() { return 'h'; }
