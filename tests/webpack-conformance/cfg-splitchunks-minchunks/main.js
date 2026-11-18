Promise.all([import('./a.js'), import('./b.js')]).then(([a, b]) => console.log(a.default, b.default));
