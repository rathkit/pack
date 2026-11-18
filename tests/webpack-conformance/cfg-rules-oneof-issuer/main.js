import raw from './t.txt?raw';
import inline from './t.txt';
console.log(raw.trim(), inline.startsWith('data:'));
