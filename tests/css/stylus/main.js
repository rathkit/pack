import './app.styl';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

console.log(css.includes('.card{color:#f00') || css.includes('.card{color:red'), css.includes('.card.title{font-weight:bold') || css.includes('.card .title{font-weight:bold'), css.includes('.sized{width:20px'));
