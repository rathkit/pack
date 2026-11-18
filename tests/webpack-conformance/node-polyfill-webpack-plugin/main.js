const path = require('path');
// path.posix, so the output is the same on every system: the plugin loads and the build runs (its polyfills are not applied, see gap 7).
console.log(path.posix.join('a', 'b'));
