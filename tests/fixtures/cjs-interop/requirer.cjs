const p = require('./plain.cjs'); const j = require('./data.json'); exports.viaRequire = p.named + ':' + j.k + ':' + (typeof require('./leaf.js').leaf);
