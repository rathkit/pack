export const early = 'E';
export function late() { return 'L:' + early; }
import('./lazy.js').then((lazy) => console.log(lazy.run()));
console.log('main done');
