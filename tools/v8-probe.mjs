// Asks V8 whether snippets parse. Reads a JSON array of strings from stdin and prints
// `ok|error<TAB>snippet` for each, parsing as a module unless MODE=script.
//
//   echo '["a = 1", "a ="]' | node --experimental-vm-modules tools/v8-probe.mjs
import vm from "node:vm";
import { readFileSync } from "node:fs";

const snippets = JSON.parse(readFileSync(0, "utf8"));
const script = process.env.MODE === "script";
for (const src of snippets) {
  let result = "ok";
  try {
    if (script) new vm.Script(src);
    else new vm.SourceTextModule(src);
  } catch (e) {
    result = `error: ${e.message}`;
  }
  console.log(`${result}\t${JSON.stringify(src)}`);
}
