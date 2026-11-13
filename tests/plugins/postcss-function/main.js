import './a.css';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
console.log(css.includes('.a{display:-fake-flex;display:flex'), css.includes('.has-cwd{x:1}'), css.includes('production'));
