import './app.less';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

console.log(css.includes('.card{color:#f00'), css.includes('.card.title{font-weight:bold'), css.includes('.rounded{border-radius:4px'), css.includes('.calc{width:30px'), !css.includes('@brand') && !css.includes('.mixin'));
