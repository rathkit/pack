const name = "w";
console.log(`a${name}b${1 + 2}`, `plain`, `multi
line ${"x"}`, `${[1, 2]}${{ toString() { return "T"; } }}`);
console.log(`outer ${`inner ${1 + 1}`}`, `${(1, 2)}`);
function tag(s, ...v) { return s.raw.join("|") + "/" + s.join("|") + "/" + v.join(","); }
console.log(tag`x${1}y${2}z`, tag`a\n${0}`, tag``);
const cache = [];
function same() { return tag`once`; }
function strings(s) { return s; }
const site = () => strings`s${1}`;
console.log(site() === site(), Object.isFrozen(site()), site().raw.length);
const o = { t(s) { return this.v + s[0]; }, v: "this:" };
console.log(o.t`x`);
