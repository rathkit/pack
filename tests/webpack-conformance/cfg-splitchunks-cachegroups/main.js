const semver = require('semver');
import('./lazy.js').then((m) => console.log(semver.valid('1.2.3'), m.default));
