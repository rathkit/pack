(function (factory) {
  if (typeof module === 'object' && typeof module.exports === 'object') {
    var v = factory(require, exports);
    if (v !== undefined) module.exports = v;
  }
})(function (require, exports) {
  var dep = require('./dep.cjs');
  var path = require('node:path');
  return { name: 'umd', sum: dep.add, viaParam: path.basename('/a/b.txt') + ':' + typeof require.cache };
});
