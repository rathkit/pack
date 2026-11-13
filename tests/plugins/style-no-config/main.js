import './theme.scss';
import './theme.less';
import './plain.css';
import box, { box as named } from './box.module.scss';
import fs from 'node:fs';

// No config file: a project with a Sass or Less compiler installed has those files compiled, and `.module.scss` has local class names.
const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
const local = /^[a-z][a-z2-7]{4}_box$/.test(box.box) && named === box.box;
console.log(css.includes('.card{color:rgb(1,2,3)'), css.includes('.panel{color:rgb(4,5,6)'), css.includes('.plain{color:rgb(7,8,9)'), css.includes(`.${box.box}{margin:7px`), local, !css.includes('$') && !css.includes('@brand'));
