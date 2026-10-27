import { ready, order, later } from './dep.js';
import { plain } from './plain.js';

const log = [];
const tick = (v) => new Promise((r) => setTimeout(() => r(v), 1));
log.push('start', plain);
const a = await tick('A');
log.push(a, ready, order.join('+'), await later());
for await (const x of (async function* () { yield 1; yield 2; })()) log.push('x' + x);
const results = await Promise.all([tick(1), tick(2)]);
log.push(results.join(','));
try {
  await Promise.reject(new Error('caught'));
} catch (e) {
  log.push(e.message);
}
const nested = async () => { await tick(0); return 'nested'; };
log.push(await nested(), typeof this);
console.log(log.join(' '));
