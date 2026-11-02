const views = import.meta.glob(['@/views/**/*.js', '!@/views/**/skip.js'], { eager: true });
console.log(Object.keys(views).sort().join(' '));
console.log(Object.keys(views).sort().map((k) => views[k].default).join(' '));
