import fs from 'node:fs';
import path from 'node:path';
// A file named by `new URL(..., import.meta.url)` is copied and the string points at the copy; a script is left alone.
const data = new URL('./data.bin', import.meta.url);
console.log(fs.readFileSync(data).length, path.basename(data.pathname).startsWith('data-'));
console.log(path.basename(new URL('./other.js', import.meta.url).pathname));
