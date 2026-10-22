import { gen, delay, run } from './lib.js';
console.log([...gen()]);
console.log(await delay(1, 'awaited'));
console.log(await run());
for await (const v of (async function* () { yield 1; yield 2; })()) console.log('av', v);
