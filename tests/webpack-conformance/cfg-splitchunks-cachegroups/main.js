import semver from 'semver';
console.log(semver.valid('1.2.3'));
import('./lazy.js').then((m) => console.log(m.default));
