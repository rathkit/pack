#!/usr/bin/env node
// Differential fuzzing of --target: random programs built from the syntax that gets rewritten (optional chains, `??`, `??=` and the other
// logical assignments, `**`, object spread and rest, classes with fields, private members, accessors and static blocks, async functions,
// `for await`, async generators and `yield*`, `super` in async methods, top-level await, numeric separators, optional catch) are run as
// they are (Node has all of it, and is the oracle) and rewritten for a target, and must print the same.
//
// Each program is a list of small snippets that run one after the other, each in its own scope with a side-effect trace; when the output
// differs, every snippet is run alone to find the ones that do.
//
//   node tools/fuzz-target.mjs [--seed N] [--programs N] [--keep] [--jobs N]
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? fallback : Number(process.argv[i + 1]);
};
const seed0 = arg('--seed', Date.now() % 1_000_000);
const programs = arg('--programs', 200);
const keep = process.argv.includes('--keep');

// ---- random numbers -----------------------------------------------------------------------------------

const rng = (seed) => {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

// ---- the part every program shares --------------------------------------------------------------------

const PRELUDE = `
const show = (v) => {
  try {
    return JSON.stringify(v, (k, x) => (typeof x === 'function' ? 'fn' : typeof x === 'symbol' ? x.toString() : typeof x === 'bigint' ? x + 'n' : x === undefined ? '__undefined' : x));
  } catch (e) {
    return 'unshowable ' + (e && e.constructor && e.constructor.name);
  }
};
const attempt = (f) => {
  try {
    return f();
  } catch (e) {
    return 'threw ' + (e && e.constructor && e.constructor.name);
  }
};
const attemptAsync = async (f) => {
  try {
    return await f();
  } catch (e) {
    return 'threw ' + (e && e.constructor && e.constructor.name);
  }
};
const run = async (label, fn) => {
  try {
    console.log(label, show(await fn()));
  } catch (e) {
    console.log(label, 'threw', e && e.constructor && e.constructor.name);
  }
};
const mkTrace = () => {
  const trace = [];
  return [trace, (x) => (trace.push(x !== null && typeof x === 'object' ? 'obj' : String(x)), x)];
};
const mkO = (trace) => ({
  a: { b: { c: 1, d: null, f() { return this && this.c === 1 ? 'this-b' : 'other'; }, arr: [1, 2, 3] } },
  n: null, u: undefined, z: 0, s: '', fn: () => 'fn',
  get g() { trace.push('get g'); return 5; }, set g(v) { trace.push('set g ' + v); },
  get h() { trace.push('get h'); return 0; }, set h(v) { trace.push('set h ' + v); },
});
`;

// ---- generators ----------------------------------------------------------------------------------------

const make = (random) => {
  const pick = (list) => list[Math.floor(random() * list.length)];
  const chance = (p) => random() < p;
  const int = (lo, hi) => lo + Math.floor(random() * (hi - lo + 1));
  const names = ['a', 'b', 'c', 'd', 'f', 'fn', 'n', 'u', 'arr', 'length', 'g', 'h', 'z', 's'];
  const value = () => pick(['t(7)', "t('v')", 't(0)', 't(null)', 't(undefined)', "t('')", 't(3)', 't(false)']);

  // optional chains
  const chain = () => {
    let e = pick(['o', 'o.a', 'o.a.b', 'o.n', 'o.u', 'n1', 'u1', 'o.fn', 'o.a.b.arr', 'o.a.b.f']);
    let lastMember = false;
    const count = int(1, 4);
    for (let i = 0; i < count; i++) {
      const optional = chance(0.5);
      switch (pick(['dot', 'dot', 'computed', 'call'])) {
        case 'dot':
          e += (optional ? '?.' : '.') + pick(names);
          lastMember = true;
          break;
        case 'computed':
          e += (optional ? '?.' : '') + `[${chance(0.5) ? `t('${pick(names)}')` : `'${pick(names)}'`}]`;
          lastMember = true;
          break;
        default:
          e += (optional ? '?.' : '') + `(${pick(['', 't(1)', 't(1), t(2)'])})`;
          lastMember = false;
      }
    }
    switch (pick(['plain', 'plain', 'paren', 'after', 'delete', 'nullish', 'cond'])) {
      case 'paren':
        return `(${e})`;
      case 'after':
        return `(${e})${pick(['.c', '.length', '?.c', '()'])}`;
      case 'delete':
        return lastMember ? `delete ${e}` : e;
      case 'nullish':
        return `${e} ?? t('fallback')`;
      case 'cond':
        return `${e} ? t('yes') : t('no')`;
      default:
        return e;
    }
  };
  const snapshot = '[n1, z1, s1, u1, o.n, o.u, o.z, o.s, o.a.b.c, o.a.b.d]';
  const header = (async = true) => `await run('SNIPPET', async () => {\n  const [trace, t] = mkTrace();\n  const o = mkO(trace);\n  let n1 = null, z1 = 0, s1 = 'x', u1;\n  const r = [];\n`;
  const footer = '  return [r, trace];\n});\n';
  const lines = (count, make1) => Array.from({ length: count }, make1).join('\n');

  const snippetChains = () => lines(int(3, 6), () => `  r.push(attempt(() => ${chain()}));`) + `\n  r.push(${snapshot});`;
  const snippetNullish = () =>
    lines(int(3, 6), () => {
      const operand = () => pick(['t(null)', 't(undefined)', 't(0)', "t('')", 't(1)', 'n1', 'o.n', 'o.z', 'u1', 'o.a.b.c', `(${chain()})`]);
      const forms = [`${operand()} ?? ${operand()}`, `(${operand()} ?? ${operand()}) || ${operand()}`, `${operand()} ?? (${operand()} && ${operand()})`, `(${operand()} || ${operand()}) ?? ${operand()}`, `${operand()} ?? ${operand()} ?? ${operand()}`];
      return `  r.push(attempt(() => ${pick(forms)}));`;
    });
  const target = () =>
    pick(['n1', 'z1', 's1', 'u1', 'o.n', 'o.u', 'o.z', 'o.s', 'o.a.b.c', 'o.a.b.d', 'o.g', 'o.h', 'o.fn', "o[t('n')]", "o[t('z')]", "o.a[t('b')][t('d')]", "o.a[t('b')].c", 'o.a.b.arr[t(0)]', 'o?.a.b.d']);
  const snippetLogical = () =>
    lines(int(4, 8), () => {
      const op = pick(['??=', '||=', '&&=', '??=', '||=', '&&=', '**=', '+=']);
      const tg = target().replace('o?.a.b.d', 'o.a.b.d');
      return `  r.push([attempt(() => (${tg} ${op} ${value()})), ${snapshot}]);`;
    }) + '\n  r.push(trace.length);';
  const snippetExponent = () =>
    lines(int(4, 8), () => {
      const n = () => pick(['t(2)', 't(3)', 't(0)', 't(-1)', 't(0.5)', 't(1)', 't(NaN)', 't(10)']);
      return `  r.push(attempt(() => ${pick([`${n()} ** ${n()}`, `${n()} ** ${n()} ** ${n()}`, `(-${n()}) ** ${n()}`, `${n()} ** -${n()}`, `(${n()} ** ${n()}) * 2`, `${n()} ** ${n()} + 1`, `-(${n()} ** ${n()})`, `2 ** 0.5`, `${n()} ** (${n()} ** ${n()})`])}));`;
    }) +
    '\n  let e1 = 2, e2 = { v: 3 }, e3 = [2];\n  r.push([attempt(() => (e1 **= t(3))), attempt(() => (e2.v **= t(2))), attempt(() => (e3[t(0)] **= t(4))), e1, e2.v, e3[0]]);';
  const source = () => pick(['o.a.b', '{ x: 1, y: 2 }', 'null', 'undefined', "'ab'", '[7, 8]', '{ get gg() { trace.push("gg"); return 1; }, y: 3 }', 'o.n', '{ ...{ p: 1 } }', 'Object.create({ inh: 1 })', '42', '{ [Symbol.for("s")]: 9, k: 1 }']);
  const snippetSpread = () =>
    lines(int(3, 6), () => {
      // Each key once: V8 orders the keys of an object literal that repeats one differently from the specification.
      const pool = [`k: t(${int(1, 5)})`, `[t('ck')]: ${int(1, 5)}`, 'x: 0', 'm() { return 1; }', 'y: 9'];
      const spreads = [`...${source()}`, `...${source()}`];
      const parts = [];
      for (let i = int(1, 4); i > 0; i--) {
        if (chance(0.5) || !pool.length) parts.push(pick(spreads));
        else parts.push(pool.splice(Math.floor(random() * pool.length), 1)[0]);
      }
      return `  r.push(attempt(() => { const v = { ${parts.join(', ')} }; return [Object.entries(v), Object.getOwnPropertySymbols(v).length]; }));`;
    }) + '\n  r.push(trace);';
  const snippetRest = () => {
    const src = '{ a: 1, b: { c: 2, d: 3, e: 4 }, e: 5, f: 6, [Symbol.for("s")]: 7 }';
    const out = [`  const src = ${src};`];
    if (chance(0.8)) out.push(`  { const { a, b: { c, ...bRest }, ...rest } = src; r.push([a, c, bRest, rest, Object.getOwnPropertySymbols(rest).length]); }`);
    if (chance(0.7)) out.push(`  { function f1({ a, ...r2 }, [x, { y, ...z }] = [1, { y: 2, w: 3 }]) { return [a, r2, x, y, z]; } r.push(f1(src), f1({ a: 0 }, [5, { y: 6, w: 7, v: 8 }])); }`);
    if (chance(0.7)) out.push(`  { const f2 = ({ a, ...r2 }) => [a, r2]; const f3 = ({ a, ...r2 }) => ({ a, r2 }); r.push(f2(src), f3({ a: 1, z: 2 })); }`);
    if (chance(0.7)) out.push(`  for (const { a, ...r3 } of [src, { a: 9, q: 1 }]) r.push([a, r3]);`);
    if (chance(0.6)) out.push(`  { const { [t('k')]: kk = t('dflt'), ...r4 } = { k2: 1, k: undefined }; r.push([kk, r4]); }`);
    if (chance(0.5)) out.push(`  { const { a = t('da'), ...r5 } = { b: 1 }; r.push([a, r5]); }`);
    if (chance(0.5)) out.push(`  r.push(attempt(() => { const { a, ...r6 } = null; return r6; }));`);
    if (chance(0.5)) out.push(`  { let { a, ...r7 } = { a: 1, b: 2 }; a++; r.push([a, r7]); }`);
    if (chance(0.4)) out.push(`  { const g = async ({ a, ...r8 }) => [a, r8]; r.push(await g(src)); }`);
    return out.join('\n') + '\n  r.push(trace);';
  };

  // classes
  let classCounter = 0;
  const snippetClass = () => {
    const id = `C${classCounter++}`;
    const has = (p) => chance(p);
    const hasX = has(0.8), hasP = has(0.8), hasM = hasP && has(0.6), hasAcc = hasP && has(0.5), hasSx = has(0.6), hasSp = has(0.5), hasSm = hasSp && has(0.5);
    const hasBlock = has(0.5), hasArrow = hasX && has(0.5), hasY = hasX && has(0.5), hasKey = has(0.4), hasQuoted = has(0.3), hasNum = has(0.2);
    const base = has(0.5);
    const ctor = base ? pick(['', 'ctor-super', 'ctor-super-after']) : pick(['', 'ctor']);
    const members = [];
    if (hasX) members.push('x = t(1);');
    if (hasY) members.push('y = this.x + 1;');
    if (hasP) members.push('#p = t(6);');
    if (hasKey) members.push("[t('ck')] = 5;");
    if (hasQuoted) members.push("'quoted-key' = 3;");
    if (hasNum) members.push('42 = 4;');
    if (hasArrow) members.push('arrow = () => this.x;');
    if (hasSx) members.push('static sx = t(2);');
    if (hasSx && has(0.5)) members.push('static sy = this.sx;');
    if (hasSp) members.push('static #sp = 7;');
    if (hasBlock) members.push(`static { trace.push('block ' + ${hasSx ? 'this.sx' : '0'}); ${hasSp ? 'trace.push(C.#sp);' : ''} }`.replace('C.#sp', `${id}.#sp`));
    if (hasM) members.push("#m() { return 'm' + this.#p; }");
    if (hasSm) members.push("static #sm() { return 'sm' + " + id + '.#sp; }');
    if (hasAcc) members.push('get #acc() { return this.#p * 2; } set #acc(v) { this.#p = v; }');
    const probe = ['  probe() {', '    const out = [];'];
    if (hasP) probe.push('    out.push(this.#p);');
    if (hasM) probe.push('    out.push(this.#m());');
    if (hasAcc) probe.push('    out.push(this.#acc); this.#acc = 11; out.push(this.#acc, this.#p);');
    if (hasP) probe.push(`    out.push(#p in this, #p in { p: 1 });`);
    if (hasP) probe.push('    this.#p++; this.#p += 2; this.#p **= 2; out.push(this.#p);');
    if (hasP && has(0.5)) probe.push('    this.#p ??= 1; this.#p ||= 2; this.#p &&= 3; out.push(this.#p);');
    if (hasP && has(0.4)) probe.push('    out.push(attempt(() => this?.#p), this?.#p?.toString());');
    if (hasX) probe.push('    out.push(this.x, this.arrow ? this.arrow() : null);');
    probe.push('    return out;', '  }');
    members.push(probe.join('\n'));
    if (hasSp) members.push(`static sprobe() { return [${id}.#sp, ${hasSm ? `${id}.#sm()` : 'null'}, #sp in ${id}]; }`);
    if (ctor === 'ctor') members.unshift('constructor(a) { trace.push("ctor " + a); }');
    if (ctor === 'ctor-super') members.unshift('constructor(a) { super(a); trace.push("ctor " + a); }');
    if (ctor === 'ctor-super-after') members.unshift('constructor(a) { trace.push("pre"); super(a); trace.push("post " + (this.bx)); }');
    const expression = has(0.3);
    const heritage = base ? ' extends B' : '';
    const declare = expression ? `  const ${id} = class${has(0.5) ? ' ' + id + 'In' : ''}${heritage} {\n${members.map((m) => '    ' + m.replace(/\n/g, '\n    ')).join('\n')}\n  };` : `  class ${id}${heritage} {\n${members.map((m) => '    ' + m.replace(/\n/g, '\n    ')).join('\n')}\n  }`;
    const classNames = 'typeof ' + id + '.name';
    return (
      `  class B { constructor(a) { trace.push('B ' + a); this.bx = 1; } static bs = 'bs'; bf = 'bf'; }\n` +
      declare +
      `\n  const inst = new ${id}(${int(1, 3)});\n  r.push([Object.keys(inst), inst.probe(), ${classNames}, Object.getOwnPropertyNames(${id}).sort()${hasSp ? `, ${id}.sprobe()` : ''}]);\n  r.push(new ${id}(9).probe());\n  r.push(trace);`
    );
  };

  // async
  const awaitFill = () => pick(['await t(1)', 'await null', 'await Promise.resolve(2)', 'await Promise.all([t(1), Promise.resolve(2)])', 'await new Promise((r2) => setTimeout(() => r2(3), 1))', 'await (async () => t(4))()']);
  const snippetAsyncOrder = () =>
    `  const ev = [];\n  const p = (async () => { ev.push('a1'); ${awaitFill()}; ev.push('a2'); await null; ev.push('a3'); ${pick(["return 'ret';", 'throw new Error("late");', 'return await t(5);'])} })();\n  ev.push('b1');\n  Promise.resolve().then(() => ev.push('m1')).then(() => ev.push('m2'));\n  ev.push('b2');\n  ev.push(await attemptAsync(() => p));\n  r.push(ev);`;
  const snippetForAwait = () => {
    const k = int(0, 3);
    const flow = pick(['', `if (v === ${k}) break;`, `if (v === ${k}) continue;`, `if (v === ${k}) return ['early', ev];`, `if (v === ${k}) throw new Error('x' + v);`]);
    const source2 = pick(['g(4)', '[10, Promise.resolve(11), 12]', 'new Set([1, 2, 3])', 'gs()', 'h()']);
    return (
      `  const ev = [];\n  async function* g(n) { try { for (let i = 0; i < n; i++) { ev.push('g' + i); yield i; } return 'done'; } finally { ev.push('g-finally'); } }\n  function* gs() { yield 1; yield Promise.resolve(2); }\n  const h = () => ({ [Symbol.asyncIterator]() { let i = 0; return { next: async () => ({ done: i >= 3, value: i++ }), return: async () => { ev.push('h-return'); return { done: true }; } }; } });\n` +
      `  r.push(await attemptAsync(async () => {\n    outer: for await (const v of ${source2}) {\n      ev.push('v' + v);\n      ${flow}\n      ${chance(0.4) ? `for await (const w of g(2)) { ev.push('w' + w); if (w === 1) continue outer; }` : ''}\n    }\n    return 'ok';\n  }));\n  r.push(ev);`
    );
  };
  const snippetAsyncGen = () =>
    `  const ev = [];\n  async function* gen() { ev.push('start'); const x = yield 1; ev.push('got ' + x); try { yield await t(2); yield* [3, 4]; yield* (async function* () { yield 5; return 'inner'; })(); } finally { ev.push('fin'); } return 'end'; }\n  const it = gen();\n  r.push(await it.next('ignored'), await it.next('X'), await it.next(), await it.next(), await it.next());\n  ${pick(["r.push(await it.return('early'), await it.next());", "r.push(await attemptAsync(() => it.throw(new Error('boom'))));", "r.push(await it.next());"])}\n  const it2 = gen(); await it2.next();\n  r.push(await attemptAsync(() => it2.throw(new Error('t2'))), await it2.next());\n  r.push(ev);`;
  const snippetAwaitPositions = () =>
    lines(int(4, 8), () => {
      const expr = pick([
        `[await t(1), ...[await t(2)]]`,
        `{ a: await t(1), [await t('k')]: await t(2), ...(await t({ s: 1 })) }`,
        `\`\${await t('x')}-\${await t('y')}\``,
        `(await t(2)) ** (await t(3))`,
        `(await t(null)) ?? (await t('d'))`,
        `o?.[await t('a')]?.[await t('b')]?.c`,
        `await t(0) ? await t('yes') : await t('no')`,
        `(await t(1)) + (await t(2)) * (await t(3))`,
        `Math.max(await t(1), await t(5), ...[await t(3)])`,
        `new Array(await t(3)).length`,
        `[await t(1), await t(2)].map((x) => x * 2)`,
        `(await Promise.all([t(1), t(2)])).reduce((s, x) => s + x, 0)`,
        `typeof (await t(1))`,
        `!(await t(0))`,
        `(await t({ q: 1 })).q`,
        `[...(await t([1, 2])), await t(3)]`,
      ]);
      return `  r.push(await attemptAsync(async () => (${expr})));`;
    }) + '\n  r.push(trace);';
  const snippetAsyncMethods = () =>
    `  const ev = [];\n  class Base { greet(x) { return 'base:' + x; } get prop() { return 'bp'; } static sgreet() { return 'sbase'; } }\n  class Derived extends Base {\n    base = 'field';\n    async greet(x) { await null; return 'derived>' + super.greet(x) + ':' + super.prop + ':' + this.base; }\n    ${pick(['async *items() { yield super.greet(1); yield await t(2); }', 'async items2() { return [super.prop, ...[super.greet(2)]]; }'])}\n    static async sgreet() { return 'ds>' + super.sgreet(); }\n    async arrow() { const f = async (y) => super.greet(y + (await t(1))); return f('a'); }\n    async args() { const f = async () => arguments.length; return [await f(), arguments[0]]; }\n    async writes() { super.wprop = await t(5); super.wprop += 1; return [this.wprop, Object.getOwnPropertyNames(this)]; }\n  }\n  const d = new Derived();\n  r.push(await d.greet('x'), await Derived.sgreet(), await d.arrow(), await d.args('A', 'B'), await d.writes());\n  if (d.items) { const out = []; for await (const v of d.items()) out.push(v); r.push(out); }\n  if (d.items2) r.push(await d.items2());\n  const obj = { async m() { return typeof this + (await t(1)); }, async *gm() { yield await t('o'); }, async 'quoted'() { return 'q'; }, async [await t('comp')]() { return 'c'; } };\n  r.push(await obj.m(), await obj.quoted(), await obj.comp(), (await obj.gm().next()).value);\n  r.push(trace);`;
  const snippetControlFlow = () =>
    `  const ev = [];\n  const f = async (mode) => {\n    try {\n      ev.push('try ' + mode);\n      if (mode === 'throw') throw new Error('t');\n      if (mode === 'return') return await t('r');\n      await null;\n    } catch (e) {\n      ev.push('catch ' + e.message);\n      return 'from-catch';\n    } finally {\n      ev.push('finally ' + mode);\n      ${pick(["await null;", "if (mode === 'override') return 'overridden';", ''])}\n    }\n    return 'end ' + mode;\n  };\n  for (const mode of ['ok', 'throw', 'return', 'override']) r.push(await f(mode));\n  ${pick(['label: for (const a of [1, 2, 3]) { for (const b of [1, 2]) { await null; if (b === 2) continue label; ev.push(a + ":" + b); } }', 'let i = 0; while (true) { i += await t(1); if (i > 2) break; } ev.push(i);', 'switch (await t(2)) { case 1: ev.push("one"); break; case await t(2): ev.push("two"); default: ev.push("fall"); }'])}\n  r.push(ev);`;
  const snippetSyntaxBits = () =>
    lines(int(3, 6), () =>
      pick([
        `  r.push(${pick(['1_000', '0b1_01', '0xFF_FF', '1_0.5_0', '.5_5', '1_0e1_0'])} + 0);`,
        `  r.push(attempt(() => { try { throw new Error('e'); } catch { return 'optional-catch'; } }));`,
        `  r.push(((a, b,) => a + b)(1, 2,));`,
        `  r.push(Math.max(1, 2, 3,));`,
        `  r.push((function f(a, b,) { return [a, b, f.length]; })(1, 2,));`,
        `  r.push(attempt(() => { try { return 1; } finally { r.push('f'); } }));`,
        `  r.push([...'ab', ...[1, 2], ...new Set([3])].length);`,
      ]),
    );

  const families = [
    ['chains', snippetChains, 3],
    ['nullish', snippetNullish, 2],
    ['logical', snippetLogical, 3],
    ['exponent', snippetExponent, 2],
    ['spread', snippetSpread, 2],
    ['rest', snippetRest, 2],
    ['class', snippetClass, 5],
    ['asyncOrder', snippetAsyncOrder, 2],
    ['forAwait', snippetForAwait, 3],
    ['asyncGen', snippetAsyncGen, 2],
    ['awaitPositions', snippetAwaitPositions, 2],
    ['asyncMethods', snippetAsyncMethods, 2],
    ['controlFlow', snippetControlFlow, 2],
    ['syntaxBits', snippetSyntaxBits, 1],
  ];
  const weighted = families.flatMap(([name, fn, weight]) => Array(weight).fill([name, fn]));
  const snippet = (index) => {
    const [name, fn] = pick(weighted);
    return { name, text: header().replace('SNIPPET', `${index}:${name}`) + fn() + '\n' + footer };
  };
  return { snippet, pick, int };
};

// ---- running ---------------------------------------------------------------------------------------------

const TARGETS = ['es2015', 'es2016', 'es2017', 'es2018', 'es2019', 'es2020', 'es2021', 'chrome80', 'safari13', 'node14', 'chrome60', 'firefox78', 'ios14.5', 'es2015', 'es2018'];
const work = mkdtempSync(join(tmpdir(), 'fuzz-target-'));

const node = (file) => spawnSync('node', ['--no-warnings', file], { encoding: 'utf8', timeout: 20000 });
const build = (dir, target, extra = []) => spawnSync(bin, ['bundle', 'main.mjs', '--outfile', join(dir, 'out.mjs'), '--target', target, ...extra], { cwd: dir, encoding: 'utf8' });

/** Writes and checks one program. Returns null when it agrees, or what differs. */
const check = (name, snippets, target, extra = []) => {
  const dir = join(work, name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'main.mjs'), PRELUDE + snippets.map((s) => s.text).join('\n'));
  const want = node(join(dir, 'main.mjs'));
  if (want.status !== 0) return { kind: 'generator', detail: want.stderr.split('\n').slice(0, 6).join('\n'), dir };
  const built = build(dir, target, extra);
  if (built.status !== 0) return { kind: 'build', detail: built.stderr.trim().split('\n')[0], dir };
  const got = node(join(dir, 'out.mjs'));
  if (got.status !== 0 || got.stdout !== want.stdout) {
    const a = want.stdout.split('\n');
    const b = got.stdout.split('\n');
    let i = 0;
    while (i < a.length && a[i] === b[i]) i++;
    return { kind: 'output', detail: `line ${i + 1}\n  expected: ${a[i]}\n  got:      ${b[i]}${got.status !== 0 ? '\n  ' + got.stderr.split('\n').slice(0, 4).join('\n  ') : ''}`, dir };
  }
  if (!keep) rmSync(dir, { recursive: true, force: true });
  return null;
};

let problems = 0;
let refused = 0;
const refusals = new Map();
for (let n = 0; n < programs; n++) {
  const seed = seed0 + n;
  const random = rng(seed);
  const g = make(random);
  const target = g.pick(TARGETS);
  const snippets = Array.from({ length: g.int(8, 14) }, (_, i) => g.snippet(i));
  const found = check(`p${seed}`, snippets, target);
  if (!found) continue;
  if (found.kind === 'build' && /cannot rewrite it yet|cannot be rewritten|cannot rewrite/.test(found.detail)) {
    // Something the program has that this target cannot be given, said so: find which snippets, and count them.
    refused++;
    const key = found.detail.replace(/^error: /, '').replace(/\(it came with es\d+\)/, '').slice(0, 140);
    refusals.set(key, (refusals.get(key) ?? 0) + 1);
    continue;
  }
  // Which snippets do it alone?
  const culprits = [];
  for (let i = 0; i < snippets.length; i++) {
    const alone = check(`p${seed}-s${i}`, [snippets[i]], target);
    if (alone) culprits.push({ snippet: snippets[i], alone });
  }
  problems++;
  console.log(`\nFAIL seed ${seed} target ${target}: ${found.kind}: ${found.detail}`);
  for (const c of culprits.slice(0, 3)) {
    console.log(`--- snippet ${c.snippet.name} (${c.alone.kind}: ${c.alone.detail})\n${c.snippet.text}`);
  }
  if (!culprits.length) console.log('(no single snippet does it; the program is kept at ' + found.dir + ')');
}
console.log(`\n${programs} programs from seed ${seed0}: ${problems} problem(s), ${refused} refused by the target`);
for (const [what, count] of refusals) console.log(`  refused ${count}x: ${what}`);
if (!keep && !problems) rmSync(work, { recursive: true, force: true });
else console.log('kept: ' + work);
process.exit(problems ? 1 : 0);
