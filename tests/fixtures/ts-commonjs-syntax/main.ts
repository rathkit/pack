import lib = require('./lib.cts');
import path = require('node:path');
import Alias = lib.Inner;
console.log(lib.add(1, 2), path.basename('/a/b.txt'), Alias.x);
