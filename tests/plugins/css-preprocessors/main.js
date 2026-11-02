import './theme.less';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8');
console.log(css.replace(/\s/g, '').includes('.card{color:rgb(1,2,3)') && !css.includes('@brand'));
