import styles, { card, token, nest } from "./app.module.css";
import shared from "./shared.module.css";
const local = (name) => typeof styles[name] === "string" && /^[a-z][a-z2-7]{4}_/.test(styles[name]);
const names = ["card", "title", "nest", "explicit", "multi", "multi2", "child", "string", "size", "spin", "responsive", "grid", "layered", "contained", "parent", "joined", "descendant", "hidden"];
// Every class of a selector is local, whatever it sits in (at-rules, nesting, :not); :global ones are not exported.
console.log(names.every(local), ["legacy", "plain", "notaclass"].every((n) => !(n in styles)));
// `@value` (its own, imported with a rename, and used in an at-rule) and `:export` are exported.
console.log([styles.token, token, styles.fixed, styles.brand, styles.space, styles.small, styles.both].join("|"));
// `composes` from another sheet and within the sheet.
console.log(styles.title.split(" ").length, styles.title.split(" ").includes(shared.base), styles.title.split(" ").includes(shared.other), nest.split(" ")[1] === card);
