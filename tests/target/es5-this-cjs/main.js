const f = () => this === module.exports;
console.log(f());
exports.x = 1;
const g = () => () => this.x;
console.log(g()());
if (true) { function early() { return 1; } console.log(early()); function other() { return early() + 1; } console.log(other()); }
