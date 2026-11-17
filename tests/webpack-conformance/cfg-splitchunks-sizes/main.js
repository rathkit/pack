const a = require('./a.js');
const b = require('./b.js');
Promise.all([import('./x.js'), import('./y.js')]).then(([x, y]) => console.log(a, b, x.default, y.default));
