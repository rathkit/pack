async function* inner() { yield 1; yield 2; return "r"; }
async function* outer() { const r = yield* inner(); yield r; yield* [8, 9]; }
(async () => { const out = []; for await (const v of outer()) out.push(v); console.log(out); })();
