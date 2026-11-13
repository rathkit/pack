import './a.scss';
import fs from 'node:fs';

// Sass runs first, so PostCSS sees `display: flex` and not `display: $d`.
const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
console.log(css.includes('.a{display:-fake-flex;display:flex'));
