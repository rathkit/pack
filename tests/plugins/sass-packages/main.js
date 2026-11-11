import './theme.scss';
import 'virtual:block';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8');
console.log(css.replace(/\s/g, '').includes('.kit{color:rgb(4,5,6)') && css.includes('.card') && css.includes('.block'));
