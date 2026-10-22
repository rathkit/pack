exports.early = 1; const b = require('./b.cjs'); exports.late = b.seen; module.exports.done = true;
