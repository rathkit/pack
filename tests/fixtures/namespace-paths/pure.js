// A call of a local function that only makes a value can go when its result is unused; one that does anything else must stay.
let count = 0;
const BASE = 10, LIMIT = BASE * 2;
function make(n) { const o = { n }; return o; }
function counted() { count++; return 1; }
function viaGlobal() { return Math.max(BASE, 3) + Math.PI; }
function reassigned() { return 1; }
const a = make(1), b = viaGlobal(), c = counted(), d = -LIMIT;
const e = reassigned();
export function report() { return [LIMIT, BASE + 1].join(','); }
export function calls() { return count; }
