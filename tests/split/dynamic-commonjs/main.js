// `import()` of CommonJS modules under splitting: they run where the call is, and only when it is made.
console.log('start');
const load = (name) => (name === 'a' ? import('./a.cjs') : import('./b.cjs'));
Promise.all([load('a'), load('b'), load('a')]).then(([a, b, again]) => {
  console.log(a.default.name, b.default.name, a.default === again.default);
  console.log(typeof a.helper, b.default.shared());
});
