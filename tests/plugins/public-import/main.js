// A script may import a file of `public/` by its address, as a Vue template's `<use href="/icons.svg#a">` does.
import icons from '/icons.svg';
import withQuery from '/icons.svg?url';
console.log(typeof icons, icons.includes('icons'), icons.endsWith('.svg'), typeof withQuery);
