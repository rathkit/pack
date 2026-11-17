const l = 'en';
const m = await import(`./locales/${l}.js`);
console.log(m.default);
