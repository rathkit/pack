#!/usr/bin/env node
// Checks that minifying a bundle in pieces (side by side, see src/minify/fragments.rs) writes
// the same text as minifying it in one pass. The one-pass side is told where the pieces end
// (RATHKIT_FRAGMENTS=whole) so it does not merge declarations across those places; apart from
// that the two must agree to the byte, for every format and with small pieces (many cuts) and
// large ones. The pieces side must also never fall back (RATHKIT_FRAGMENTS=strict).
//
//   node tools/check-pieces.mjs [--packages PROJECT] [--limit N]
//
// Entries are the programs under tests/fixtures and, with --packages, a driver for every package
// in PROJECT/node_modules.
import { spawnSync } from "node:child_process";
import { readdirSync, writeFileSync, mkdirSync, rmSync, existsSync, statSync, readFileSync, mkdtempSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, "target", "release", "rathkit-pack");
const flag = (name) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? null : process.argv[i + 1];
};
const limit = Number(flag("--limit") ?? 1e9);

const entries = [];
const fixtures = join(root, "tests", "fixtures");
for (const name of readdirSync(fixtures)) {
  const dir = join(fixtures, name);
  if (!statSync(dir).isDirectory()) continue;
  const entry = ["main.ts", "main.tsx", "main.js"].find((f) => existsSync(join(dir, f)));
  if (entry) entries.push({ name, file: entry, cwd: dir, extra: [] });
}
const work = mkdtempSync(join(tmpdir(), "pieces-"));
const project = flag("--packages");
// Drivers live inside the project so that packages resolve from them.
const drivers = project ? join(project, ".rathkit-pieces-drivers") : null;
if (drivers) {
  rmSync(drivers, { recursive: true, force: true });
  mkdirSync(drivers);
}
if (project) {
  const modules = join(project, "node_modules");
  const names = [];
  for (const entry of readdirSync(modules)) {
    if (entry.startsWith(".")) continue;
    if (entry.startsWith("@")) for (const sub of readdirSync(join(modules, entry))) names.push(`${entry}/${sub}`);
    else names.push(entry);
  }
  for (const name of names.slice(0, limit)) {
    const driver = join(drivers, name.replace("/", "__") + ".mjs");
    writeFileSync(driver, `import * as m from ${JSON.stringify(name)};\nconsole.log(Object.keys(m).length);\n`);
    entries.push({ name, file: driver, cwd: project, extra: ["--condition", "node", "--main-field", "main", "--original-paths"] });
  }
}

// Packages are many and large, so each gets fewer combinations than a fixture does.
const formats = project ? [[], ["--format", "iife", "--global-name", "G"]] : [[], ["--format", "cjs"], ["--format", "iife"], ["--format", "iife", "--global-name", "G"]];
const modes = project ? [["--minify"], ["--minify", "--keep-names"]] : [["--minify"], ["--minify", "--keep-names"], ["--minify-identifiers"], ["--minify-syntax"]];
const sizes = project ? ["20000"] : ["1", "5000"];
const build = (entry, format, mode, size, how) => {
  const out = join(work, "out.js");
  const b = spawnSync(bin, ["bundle", entry.file, "--outfile", out, ...entry.extra, ...format, ...mode], {
    cwd: entry.cwd,
    encoding: "utf8",
    env: { ...process.env, RATHKIT_FRAGMENTS: how, RATHKIT_FRAGMENT_BYTES: size },
    timeout: 120000,
    maxBuffer: 1 << 28,
  });
  if (b.status !== 0) return { error: b.stderr.split("\n").find((l) => /error/.test(l)) ?? b.stderr.slice(0, 200) };
  return { text: readFileSync(out, "utf8") };
};
// A `;` that would end the file is written when pieces follow each other; one pass only drops it
// when everything after it was left out too. Either way the program is the same.
const norm = (t) => t.replace(/;\s*$/, "");

let compared = 0, failures = 0, skipped = 0;
// With packages, one more entry imports every package that bundles alone: a bundle of hundreds of
// modules, cut into many pieces.
const bundles = (entry) => !build(entry, [], ["--minify"], "20000", "whole").error;
if (project) {
  const usable = entries.filter((e) => e.extra.length && bundles(e));
  const driver = join(drivers, "all-packages.mjs");
  writeFileSync(driver, usable.map((e, i) => `import * as m${i} from ${JSON.stringify(e.name)};`).join("\n") + `\nconsole.log(${usable.map((_, i) => `Object.keys(m${i}).length`).join(" + ") || 0});\n`);
  entries.push({ name: `all ${usable.length} packages`, file: driver, cwd: project, extra: ["--condition", "node", "--main-field", "main", "--original-paths"] });
}
for (const entry of entries) {
  for (const format of formats) {
    for (const mode of modes) {
      for (const size of sizes) {
        const one = build(entry, format, mode, size, "whole");
        if (one.error) {
          skipped++;
          continue;
        }
        const pieces = build(entry, format, mode, size, "strict");
        compared++;
        let problem = null;
        if (pieces.error) problem = `pieces failed: ${pieces.error}`;
        else if (norm(one.text) !== norm(pieces.text)) {
          let i = 0;
          while (i < one.text.length && one.text[i] === pieces.text[i]) i++;
          problem = `texts differ at byte ${i}:\n   one pass: ${JSON.stringify(one.text.slice(Math.max(0, i - 60), i + 80))}\n   pieces:   ${JSON.stringify(pieces.text.slice(Math.max(0, i - 60), i + 80))}`;
        }
        if (problem) {
          failures++;
          if (failures <= 8) console.log(`FAIL ${entry.name} [${format.join(" ")} ${mode.join(" ")} bytes=${size}]: ${problem}`);
        }
      }
    }
  }
}
rmSync(work, { recursive: true, force: true });
if (drivers) rmSync(drivers, { recursive: true, force: true });
console.log(`${compared} builds compared, ${failures} differences (${skipped} skipped: the bundle itself failed)`);
process.exit(failures ? 1 : 0);
