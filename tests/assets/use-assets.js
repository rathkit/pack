import logo from './logo.svg';
import pic from './pic.png';
import font from './f.woff2';
const kind = (v) => (v.startsWith('data:') ? 'inlined' : 'file ' + v.replace(/^.*\//, '').replace(/[0-9a-f]{8,}/, 'HASH'));
console.log('svg', kind(logo));
console.log('png', kind(pic));
console.log('font', kind(font));
