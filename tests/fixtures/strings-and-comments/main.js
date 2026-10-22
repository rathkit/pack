import { s } from './lib.js';
// import { fake } from './nope.js';
/* export const fake = 1; */
const text = "import { x } from 'y'; export default 1";
const re = /import\s+(\w+)/g;
console.log(s, text, 'import z'.replace(re, '$1'), `export ${s}`);
