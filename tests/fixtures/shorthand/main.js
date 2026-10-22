import { name, age, helper } from './lib.js';
const name2 = 'local';
const o = { name, age, helper, name2 };
const { name: n1, age: age2 = 5, ...rest } = { name: 'x', extra: 1 };
let name3;
({ name3 = 'dflt' } = {});
function fn({ name, age }) { return name + age; }
console.log(o.name, o.age, o.helper(), n1, age2, rest, name3, fn({ name: 'p', age: 1 }));
const { helper: h2 = () => 'fallback' } = {};
console.log(h2());
