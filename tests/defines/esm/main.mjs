import { devtools } from "./devtools.mjs";
import { helper } from "./helper.mjs";

console.log("helper", helper());
if (process.env.NODE_ENV === "production") {
  console.log("prod only");
} else {
  const dev = await import("./dev.mjs");
  console.log("dev", dev.default);
}
const later = process.env.NODE_ENV === "production" ? null : await import("./dev.mjs");
console.log("later", later === null ? "none" : later.default);
console.log(process.env.NODE_ENV !== "production" && "has devtools");
if (process.env.NODE_ENV !== "production") devtools();
const tools = process.env.NODE_ENV === "production" ? null : devtools;
console.log(typeof tools);
