import env from 'env';
import virtual from 'virtual:thing';
import text from './notes.glob';
console.log(JSON.stringify(env), virtual, text);
