import html, { resource } from './doc.md';
import bytes from './data.bin';
console.log(html, resource, bytes.join(','), process.env.MODE, FLAG);
