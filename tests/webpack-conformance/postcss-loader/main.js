const css = require('./a.css').default.toString();
console.log(css.includes('-webkit-user-select') || css.includes('-webkit-box'), css.includes('display: flex'));
