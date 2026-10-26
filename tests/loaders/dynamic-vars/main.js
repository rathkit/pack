// Variable dynamic imports: the path starts with ./, a variable stands for one name, the extension is written out.
const show = async (label, load) => {
  try {
    console.log(label, await load());
  } catch (e) {
    console.log(label, 'error:', e.message.replace(/ at .*/s, '').slice(0, 60));
  }
};
const names = ['a', 'b'];
for (const n of names) await show('template ' + n, () => import(`./mods/${n}.js`).then((m) => m.default));
await show('concatenation', () => import('./mods/' + names[1] + '.js').then((m) => m.default));
await show('two directories', () => import(`./${'mods'}/${'sub'}/${'c'}.js`).then((m) => m.default));
await show('not in the folder', () => import(`./mods/${'missing'}.js`));
await show('a name does not cross a /', () => import(`./mods/${'sub/c'}.js`));
const text = "import(`./mods/${x}.js`)"; // import(`./mods/${y}.js`)
console.log('quoted import is text:', text.length > 0);
