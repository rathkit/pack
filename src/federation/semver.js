// Version ranges as npm writes them: `^1.2.3`, `~1.2`, `1.x`, `>=1 <3`, `1.2.3 - 2.0.0`, `a || b`.
// A version with a pre-release tag only satisfies a range that names a pre-release of the same
// major, minor and patch, as npm's rules say.

const VERSION = /^\s*[=v]*(\d+)(?:\.(\d+|[xX*]))?(?:\.(\d+|[xX*]))?(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?\s*$/;

function parse(text) {
  const m = VERSION.exec(String(text));
  if (!m) return null;
  const part = (s) => (s === undefined || s === 'x' || s === 'X' || s === '*' ? null : Number(s));
  return { major: Number(m[1]), minor: part(m[2]), patch: part(m[3]), pre: m[4] ? m[4].split('.') : [] };
}

function compareIdentifiers(a, b) {
  const an = /^\d+$/.test(a), bn = /^\d+$/.test(b);
  if (an && bn) return Number(a) - Number(b);
  if (an) return -1;
  if (bn) return 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

function compare(a, b) {
  for (const key of ['major', 'minor', 'patch']) {
    if (a[key] !== b[key]) return a[key] < b[key] ? -1 : 1;
  }
  if (!a.pre.length && !b.pre.length) return 0;
  if (!a.pre.length) return 1;
  if (!b.pre.length) return -1;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    if (a.pre[i] === undefined) return -1;
    if (b.pre[i] === undefined) return 1;
    const c = compareIdentifiers(a.pre[i], b.pre[i]);
    if (c) return c;
  }
  return 0;
}

const full = (major, minor, patch, pre = []) => ({ major, minor, patch, pre });
const text = (v) => `${v.major}.${v.minor}.${v.patch}${v.pre.length ? '-' + v.pre.join('.') : ''}`;
// The lowest version of a release line, which sorts below its own pre-releases.
const floor = (major, minor, patch) => ({ op: '<', v: full(major, minor, patch, ['0']) });
const atLeast = (v) => ({ op: '>=', v });
const none = { op: '>=', v: full(0, 0, 0) };

// One comparator in a range, without a hyphen or caret: the pieces it stands for.
function desugar(token) {
  const m = /^(<=|>=|<|>|=|\^|~>?|)\s*(.*)$/.exec(token);
  const op = m[1] === '~>' ? '~' : m[1];
  const operand = m[2].trim();
  // A bare `*` or `x` is any version: no bound to put on it.
  if (operand === '' || /^[xX*]$/.test(operand)) return operand === '' && op !== '' ? null : /^(<|>)$/.test(op) ? null : [none];
  const v = parse(operand);
  if (!v) return null;
  const { major, minor, patch, pre } = v;
  const wild = minor === null ? 2 : patch === null ? 1 : 0;
  switch (op) {
    case '^': {
      const n = minor ?? 0, p = patch ?? 0;
      let hi;
      if (major > 0) hi = floor(major + 1, 0, 0);
      else if (minor === null) hi = floor(1, 0, 0);
      else if (n > 0) hi = floor(0, n + 1, 0);
      else if (patch === null) hi = floor(0, 1, 0);
      else hi = floor(0, 0, p + 1);
      return [atLeast(full(major, n, p, pre)), hi];
    }
    case '~':
      if (wild === 2) return [atLeast(full(major, 0, 0)), floor(major + 1, 0, 0)];
      if (wild === 1) return [atLeast(full(major, minor, 0)), floor(major, minor + 1, 0)];
      return [atLeast(full(major, minor, patch, pre)), floor(major, minor + 1, 0)];
    case '>':
      if (wild === 2) return [atLeast(full(major + 1, 0, 0))];
      if (wild === 1) return [atLeast(full(major, minor + 1, 0))];
      return [{ op: '>', v: full(major, minor, patch, pre) }];
    case '>=':
      return [atLeast(full(major, minor ?? 0, patch ?? 0, pre))];
    case '<':
      return wild === 0 ? [{ op: '<', v: full(major, minor, patch, pre) }] : [floor(major, minor ?? 0, patch ?? 0)];
    case '<=':
      if (wild === 2) return [floor(major + 1, 0, 0)];
      if (wild === 1) return [floor(major, minor + 1, 0)];
      return [{ op: '<=', v: full(major, minor, patch, pre) }];
    default:
      if (wild === 2) return [atLeast(full(major, 0, 0)), floor(major + 1, 0, 0)];
      if (wild === 1) return [atLeast(full(major, minor, 0)), floor(major, minor + 1, 0)];
      return [{ op: '=', v: full(major, minor, patch, pre) }];
  }
}

function parseSet(set) {
  let s = set.trim();
  // `1.2.3 - 2.3.4` is `>=1.2.3 <=2.3.4`; a partial end is read as the whole line it names.
  s = s.replace(/^\s*(\S+)\s+-\s+(\S+)\s*$/, (_, from, to) => `>=${from} <=${to}`);
  // `>= 1.2.3` is `>=1.2.3`.
  s = s.replace(/(<=|>=|<|>|=|\^|~>?)\s+/g, '$1');
  const comparators = [];
  for (const token of s.split(/\s+/).filter(Boolean)) {
    const parts = desugar(token);
    if (!parts) return null;
    comparators.push(...parts);
  }
  return comparators.length ? comparators : [none];
}

function holds(c, version) {
  const d = compare(version, c.v);
  switch (c.op) {
    case '>': return d > 0;
    case '>=': return d >= 0;
    case '<': return d < 0;
    case '<=': return d <= 0;
    default: return d === 0;
  }
}

export function satisfy(version, range) {
  const v = parse(version);
  if (!v || v.minor === null || v.patch === null) return false;
  for (const set of String(range).split('||')) {
    const comparators = parseSet(set);
    if (!comparators || !comparators.every((c) => holds(c, v))) continue;
    // A pre-release only counts where the range mentions one of the same release.
    if (v.pre.length && !comparators.some((c) => c.v.pre.length && c.v.major === v.major && c.v.minor === v.minor && c.v.patch === v.patch)) continue;
    return true;
  }
  return false;
}

// Whether `a` sorts before `b`; versions that do not parse sort first.
export function lessThan(a, b) {
  const x = parse(a), y = parse(b);
  if (!x || x.minor === null || x.patch === null) return !!y;
  if (!y || y.minor === null || y.patch === null) return false;
  return compare(x, y) < 0;
}

export { text as formatVersion };
