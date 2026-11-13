import './a.css';
import b, { b as named } from './b.module.css';
import fs from 'node:fs';

// A PostCSS config next to the project, no rkp config: plugins are loaded by name with their options (and one set to false is not loaded),
// they run on a plain sheet and on a `.module.css`, and the module sheet keeps its local names.
const css = fs.readFileSync(process.env.BUNDLE.replace(/\.(mjs|cjs)$/, '.css'), 'utf8').replace(/\s/g, '');
console.log(css.includes('.a{display:-fake-flex;display:flex'), css.includes(`.${b.b}{display:-fake-flex;display:flex`), /^[a-z][a-z2-7]{4}_b$/.test(b.b) && named === b.b, css.includes('.banner{content:"fromconfig"'));
