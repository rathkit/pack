import './theme.less';
import fs from 'node:fs';

const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8');
console.log(/\.card\{color:(rgb\(1,2,3\)|#010203)/.test(css.replace(/\s/g, "")) && !css.includes('@brand'));
