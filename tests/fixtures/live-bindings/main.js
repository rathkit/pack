import { count, inc, obj, setObj, items } from './state.js';
console.log(count); inc(); inc(); console.log(count);
console.log(obj.v); setObj({ v: 2 }); console.log(obj.v);
console.log(items.length);
