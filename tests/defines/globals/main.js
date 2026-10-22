const results = [];
const show = (label, value) => results.push(label + "=" + JSON.stringify(value));

show("flag", __FLAG__);
show("not", !__FLAG__);
show("typeof", typeof __FLAG__);
show("count", __COUNT__ + 1);
show("method", __COUNT__.toFixed(1));
show("text", __TEXT__.toUpperCase());
show("config", CONFIG.name + CONFIG.list.length);
show("alias", typeof ROOT.Array);
show("nothing", __NOTHING__ === undefined);
show("nullish", __NULL__ ?? "fallback");
show("and", __FLAG__ && "yes");
show("or", __FLAG__ || "no");
show("shorthand", { __FLAG__, __COUNT__ });
show("ternary", __FLAG__ ? "on" : "off");
show("compare", __TEXT__ === "hello");
show("loose", __COUNT__ == 3);

function inner(__FLAG__) {
  // a parameter of the same name is not the global
  return __FLAG__ ? "param true" : "param false";
}
show("shadow", inner(false));

if (__FLAG__) {
  show("branch", "flag on");
} else {
  show("branch", "flag off");
}

if (!__FLAG__) {
  var hoisted = "declared in a branch that never runs";
}
show("hoisted", typeof hoisted);

const wrapped = (function () {
  return __COUNT__ > 2 ? "big" : "small";
})();
show("wrapped", wrapped);

console.log(results.join("\n"));
console.log(require("./cjs.cjs"));
