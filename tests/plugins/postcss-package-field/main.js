import './a.css';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
console.log(css.includes('.banner{content:"frompackage.json"'), /\.a\s*\{\s*color:\s*red/.test(css));
