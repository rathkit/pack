import styles, { box } from './box.module.scss';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

console.log(/^[a-z][a-z2-7]{4}_box$/.test(box), styles.title.split(' ').length === 2, css.includes('.' + box + ':hover{'), css.includes('.' + box + '.' + styles.inner.split(' ')[0] + '{'), !css.includes('&'));
