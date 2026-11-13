const path = require('node:path');
exports.here = path.basename(__dirname) + ':' + path.basename(__filename);
