const b = [1, 2, 3];
console.log(Math.max(...b), [0, ...b, 9], [...b, ...b].length, [...new Set([1, 1, 2])], [..."ab"]);
const o = { f(...args) { return args.length + ":" + this.tag; }, tag: "T" };
console.log(o.f(...b, 4), o.f(...[]), (0, o.f).call({ tag: "U" }, ...b));
console.log(new Date(...[2020, 1, 1]).getFullYear());
class P { constructor(...a) { this.a = a; } }
console.log(new P(...[1, 2, 3]).a.length, new P(0, ...[1]).a);
function make() { return o; }
console.log(make().f(...b));
console.log([1, , 3, ...[4]].length);
