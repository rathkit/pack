import './a.scss';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
console.log(css.includes('.a{color:rgb(1,2,3)'), css.includes('.inline{x:1'));
