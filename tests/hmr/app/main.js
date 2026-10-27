import defaultFn, { n, inc, answer, Counter, kind as aKind, alias, helper, deep as reexported } from './a.js';
import * as ns from './a.js';
import Foo, { D } from './defaults.js';
import { even, odd } from './cycle-x.js';
import data from './data.json';
import cjs, { named as cjsNamed } from './lib.cjs';
import { Color, describe, Box, Util } from './typed.ts';
import logo from './logo.svg';
import { shadow } from './shadow.js';

const out = (...a) => console.log(...a.map((x) => (typeof x === 'object' && x !== null ? JSON.stringify(x) : String(x))));

out('this', typeof this);
out('default', defaultFn(), Foo.name === undefined ? 'noname' : 'named', new Foo().hello(), D);
out('live', n); inc(); inc(); out('live after', n, ns.n);
out('values', answer, new Counter(3).next(), aKind, alias, helper(2), reexported);
out('namespace', Object.keys(ns).sort().join(','), ns[Symbol.toStringTag], typeof ns.inc);
out('called without this', (function () { return this; }).call(undefined) === undefined, (0, ns.whoami)() === undefined);
out('shorthand', { n, answer }, [n, answer], `${answer}${n}`);
out('typeof', typeof answer, typeof missing, typeof inc);
out('cycle', even(10), odd(7));
out('json', data.name, data.list.length);
out('cjs', cjs.kind, cjsNamed(), cjs.kind === cjsNamed.kind);
out('ts', Color.Green, Color[1], describe({ id: 7 }), new Box(5).get());
out('shadow', shadow(100), shadow.length);
const { answer: renamed, ...rest } = { answer, other: 1 };
out('destructure', renamed, rest);
class Child extends Counter { more() { return this.next() + answer; } }
out('extends', new Child(1).more());
function params(x = answer, { n: local = n } = {}) { return [x, local]; }
out('defaults', params(), params(1, { n: 2 }));
import('./lazy.js').then((m) => { out('dynamic', m.lazy(), m.default); return import('./lib.cjs'); }).then((m) => out('dynamic cjs', m.default.kind, m.named === undefined));
(async () => { for await (const v of (async function* () { yield answer; yield n; })()) out('async iteration', v); })();
out('assets', String(logo).replace(/\\/g, '/').split('/').pop().replace(/[-.].*/, ''), new URL('./logo.svg', import.meta.url).href.split('/').pop().replace(/[-.].*/, ''));
out('namespace ts', Util.twice(4), Util.name());
out('env', typeof import.meta.env, import.meta.env && typeof import.meta.env.MODE);
