import a from './a.module.css';
import b from './b.module.css';
const parts = a.title.split(' ');
console.log(parts.length, parts.includes(b.surface), parts.includes(a.pad), /^[a-z2-7]{5}_title$/.test(parts[0]));
console.log(a.also.split(' ').filter((n) => n === b.surface || n === b.raised).length);
