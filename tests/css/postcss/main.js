import './app.css';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

// autoprefixer adds the prefix the browserslist asks for, and postcss-nested flattens the nesting.
console.log(css.includes('.a{-webkit-user-select:none') || css.includes('-webkit-user-select:none'), css.includes('.a.b{color:red'), css.includes('.a:hover{color:blue') || css.includes('.a:hover{'));
