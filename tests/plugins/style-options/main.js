import './a.scss';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
console.log(/\.a\{color:(rgb\(1,2,3\)|#010203)/.test(css), css.includes('.inline{x:1'));
