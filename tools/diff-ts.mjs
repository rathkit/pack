// Compares the TypeScript parser's verdicts with the TypeScript compiler's own.
//   rathkit-pack --ts-verdicts FILE... > ours.txt; node tools/diff-ts.mjs files.txt ours.txt
// files.txt lists the files (one per line), ours.txt has one verdict per file in the same order.
// The oracle is `ts.createSourceFile(...).parseDiagnostics`: syntax errors only, no type checking.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(process.env.TS_FROM ?? import.meta.url);
const ts = require(process.env.TS_MODULE ?? 'typescript');
const files = readFileSync(process.argv[2], 'utf8').split('\n').filter(Boolean);
const ours = readFileSync(process.argv[3], 'utf8').split('\n').filter(Boolean);
if (files.length !== ours.length) { console.log(`count mismatch: ${files.length} files, ${ours.length} verdicts`); process.exit(2); }
const counts = { bothAccept: 0, bothReject: 0, weRejectValid: 0, weAcceptInvalid: 0 };
const shown = { weRejectValid: 0, weAcceptInvalid: 0 };
const reasons = new Map();
for (let i = 0; i < files.length; i++) {
  if (ours[i] === 'SKIP') continue;
  const file = files[i];
  const text = readFileSync(file, 'utf8');
  const kind = file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, kind);
  const valid = sf.parseDiagnostics.length === 0;
  const accepted = ours[i] === 'OK';
  if (valid && accepted) counts.bothAccept++;
  else if (!valid && !accepted) counts.bothReject++;
  else if (valid) {
    counts.weRejectValid++;
    const why = ours[i].replace(/@\d+$/, '').replace(/`[^`]*`/g, '`…`').slice(0, 80);
    reasons.set(why, (reasons.get(why) ?? 0) + 1);
    if (shown.weRejectValid++ < (Number(process.env.SHOW) || 12)) console.log(`REJECTED VALID ${file}\n    ${ours[i]}`);
  } else {
    counts.weAcceptInvalid++;
    if (shown.weAcceptInvalid++ < 8) console.log(`ACCEPTED INVALID ${file}\n    tsc: ${ts.flattenDiagnosticMessageText(sf.parseDiagnostics[0].messageText, ' ')} @${sf.parseDiagnostics[0].start}`);
  }
}
console.log(counts);
console.log('top rejection reasons:', [...reasons].sort((a, b) => b[1] - a[1]).slice(0, 15));
