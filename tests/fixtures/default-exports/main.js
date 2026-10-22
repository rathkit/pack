import f1 from './anon-fn.js';
import f2 from './named-fn.js';
import C1 from './anon-class.js';
import C2 from './named-class.js';
import v from './expr.js';
import arrow from './arrow.js';
import { default as viaRe } from './re.js';
import reDefault, { other } from './re.js';
console.log(f1(), f2(), new C1().hi(), new C2().hi(), v, arrow(), viaRe(), reDefault === viaRe, other);
