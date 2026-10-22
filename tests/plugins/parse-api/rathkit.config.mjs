// A plugin that reads code with `this.parse` and cuts out `debugLog(...)` statements by their
// start and end offsets, the way magic-string based plugins do. Offsets count UTF-16 units, so the
// emoji and accents before the calls would shift them if they were counted in bytes.
const stripDebug = {
  name: 'strip-debug',
  transform(code, id) {
    if (!id.endsWith('.js')) return null;
    const tree = this.parse(code);
    const cuts = [];
    const walk = (node) => {
      if (Array.isArray(node)) return node.forEach(walk);
      if (!node || typeof node.type !== 'string') return;
      if (node.type === 'ExpressionStatement' && node.expression.type === 'CallExpression' && node.expression.callee.name === 'debugLog') cuts.push([node.start, node.end]);
      for (const key of Object.keys(node)) if (typeof node[key] === 'object') walk(node[key]);
    };
    walk(tree);
    if (!cuts.length) return null;
    let out = code;
    for (const [start, end] of cuts.sort((a, b) => b[0] - a[0])) out = out.slice(0, start) + out.slice(end);
    return { code: out, map: null };
  },
};

// Shapes other plugins rely on.
const inspect = {
  name: 'inspect',
  buildEnd() {
    const tree = this.parse('import a, { b as c } from "m"; export * as ns from "n"; export default class extends B { static #p = 1; get x() { return a?.b ?? 1; } }; label: for (const x of y) { break label; } `t${1}u`; /re+/g; 10n;');
    const types = [];
    const walk = (node) => {
      if (Array.isArray(node)) return node.forEach(walk);
      if (!node || typeof node.type !== 'string') return;
      types.push(node.type);
      for (const key of Object.keys(node)) if (typeof node[key] === 'object') walk(node[key]);
    };
    walk(tree);
    let message = 'none';
    try { this.parse('let = ;'); } catch (e) { message = `${e.code} at ${e.pos} line ${e.loc.line}`; }
    globalThis.__parseReport = `${new Set(types).size} ${tree.sourceType} ${tree.body[0].specifiers.map((s) => s.type).join('+')} ${message}`;
    process.stderr.write('');
  },
  async closeBundle() {
    const fs = await import('node:fs');
    fs.writeFileSync(`${process.env.OUT_DIR}/hooks.log`, globalThis.__parseReport + '\n');
  },
};

export default { plugins: [stripDebug, inspect] };
