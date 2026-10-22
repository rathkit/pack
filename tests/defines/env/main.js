const { mode, feature } = require("./lib.js");
const config = require("./config.json");

console.log("mode", mode());
console.log("feature", feature());

if (process.env.NODE_ENV === "production") {
  console.log("loaded", require("./prod.cjs"));
} else if (process.env.NODE_ENV === "test") {
  console.log("loaded", require("./test.cjs"));
} else {
  console.log("loaded", require("./dev.cjs"));
}

const picked = process.env.NODE_ENV !== "production" ? require("./dev-only.cjs") : "nothing";
console.log("picked", picked);

const lazy = process.env.NODE_ENV !== "production" && import("./unreachable-dynamic.js");
console.log("lazy", typeof lazy);

console.log("config", config.name, process.env.NODE_ENV, typeof process.env.NODE_ENV);
console.log(process.env["NODE_ENV"] ?? "unset", process.env.OTHER_VARIABLE_NOT_DEFINED === undefined);
