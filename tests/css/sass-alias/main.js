import './app.scss';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

// A `~/` alias is not a package named "": the name goes to the alias lookup and the partial is found.
console.log(css.includes('.tilde{color:#0a0') || css.includes('.tilde{color:green'), !css.includes('@use'));
