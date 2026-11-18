const html = require('./t.html');
console.log(/<p>hi<\/p><img src="[^"]*\.png">/.test(html) || html);
