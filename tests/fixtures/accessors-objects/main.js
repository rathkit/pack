import { box, Counter } from './lib.js';
console.log(box.val, (box.val = 5, box.val), new Counter().next().next().n);
const { val } = box;
console.log(val);
