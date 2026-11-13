import './app.scss';
import './indented.sass';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

// Variables, nesting, a partial through @use with a namespace, a mixin and a function, and the indented syntax.
console.log(css.includes('.card{color:#f00'), css.includes('.card.title{font-weight:bold') || css.includes('.card .title{font-weight:bold'), css.includes('.card:hover{'), css.includes('.pad{padding:12px'), css.includes('.indented{margin:3px'), !css.includes('$') && !css.includes('@use'));
