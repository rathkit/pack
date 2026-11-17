const css = require('./a.css');
console.log(css.default.toString().includes('body{color:red}'), css.default.toString().includes('/*'));
