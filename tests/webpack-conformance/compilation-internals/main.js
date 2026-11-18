const a = require('./a.js');
import('./lazy.js').then((m) => console.log(a, m.default));
