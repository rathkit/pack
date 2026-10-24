const lazy = import.meta.glob('./mods/*.{js,ts}');
const deep = import.meta.glob(['./mods/**/*.js', '!./mods/skip.js'], { eager: true, import: 'default' });
const raw = import.meta.glob('/other/*.txt', { query: '?raw', import: 'default', eager: true });
const named = import.meta.glob('./mods/*.js', { import: 'n' });
console.log(Object.keys(lazy), Object.keys(deep), deep, raw);
console.log(await lazy['./mods/b.ts']().then((m) => m.default), await named['./mods/a.js']());
