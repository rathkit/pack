const css = require('./a.less').default.toString();
console.log(css.includes('.a .b'), css.includes('color: blue'));
