import './theme.scss';
import 'virtual:block';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8');
console.log(/\.kit\{color:(rgb\(4,5,6\)|#040506)/.test(css.replace(/\s/g, "")) && css.includes('.card') && css.includes('.block'));
