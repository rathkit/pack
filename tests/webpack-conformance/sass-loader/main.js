const css = require('./a.scss').default.toString();
console.log(css.includes('.a .b'), /color:\s*red/.test(css));
