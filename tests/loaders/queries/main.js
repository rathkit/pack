// A query on an import says how to read the file (as in Vite): ?raw its text, ?url the address of a copy, ?inline a data URL.
import logoUrl from './logo.svg';
import logoRaw from './logo.svg?raw';
import logoUrlQuery from './logo.svg?url';
import logoInline from './logo.svg?inline';
import noteRaw from './note.txt?raw';
import noteTextByDefault from './note.txt';
console.log(logoUrl === logoUrlQuery, /^logo-[a-z0-9]+\.svg$/.test(logoUrl.split('/').pop()));
console.log(logoRaw.trim().startsWith('<svg'), logoRaw.length);
console.log(logoInline.startsWith('data:image/svg+xml;base64,'));
console.log(JSON.stringify(noteRaw), noteTextByDefault === noteRaw);
