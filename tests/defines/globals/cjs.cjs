const out = [];
if (__FLAG__) out.push("cjs flag on"); else out.push("cjs flag off");
out.push(typeof __TEXT__, __COUNT__ * 2);
module.exports = out.join(", ");
