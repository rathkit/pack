// Differential test: asks V8 (through Node) whether each file is valid JavaScript, as a
// module or as a script/CommonJS body, and prints `path<TAB>module|script|fail` lines in
// the same format as `rathkit-pack --parse-verdicts`. Diff the two outputs to find files
// the two parsers disagree on.
//
//   node --experimental-vm-modules tools/diff-v8.mjs files.txt > v8.tsv
import { readFileSync } from "node:fs";
import vm from "node:vm";

const files = readFileSync(process.argv[2], "utf8").split("\n").filter(Boolean);

// MODE=script asks only about sloppy scripts, which a valid module would otherwise hide.
const scriptOnly = process.env.MODE === "script";

function verdict(src) {
  if (!scriptOnly) {
    try {
      new vm.SourceTextModule(src);
      return "module";
    } catch {}
  }
  try {
    new vm.Script(src);
    return "script";
  } catch {}
  try {
    // CommonJS bodies may `return` at the top level.
    vm.compileFunction(src, []);
    return "script";
  } catch {}
  return "fail";
}

const out = [];
for (const file of files) {
  let src;
  try {
    src = readFileSync(file, "utf8");
  } catch {
    continue;
  }
  out.push(`${file}\t${verdict(src)}`);
}
process.stdout.write(out.join("\n") + "\n");
