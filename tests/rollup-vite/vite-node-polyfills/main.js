import path from 'path';
// path.posix, so the output is the same on every system: this shows that the plugin loads and the Node build runs (see the table for browser builds).
console.log(path.posix.join('a', 'b'));
