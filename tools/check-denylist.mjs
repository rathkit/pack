// Scans the repository for words that must never appear in it (the private project plan, names of other products that are not ours to mention).
// The word list is NOT stored in the repo. It comes from the RATHKIT_DENYLIST environment variable (comma or newline separated), set from
// CI secrets; the same name is used by the other Rathkit repositories. (This repository names other bundlers on
// purpose, in the benchmark figures and in what plugins it runs; the list is of words that are not allowed anywhere in it.)
//   node tools/check-denylist.mjs            the files now
//   node tools/check-denylist.mjs --history  every commit on every branch and tag: messages, author names and every added line and path
// (the second is for the day before a repository is made public, when history becomes public too).
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const split = (value) =>
  (value ?? "")
    .split(/[,\n]/)
    .map((word) => word.trim().toLowerCase())
    .filter(Boolean);

const denied = split(process.env.RATHKIT_DENYLIST);

if (denied.length === 0) {
  if (process.env.CI) {
    console.error("denylist: RATHKIT_DENYLIST is not set; refusing to pass in CI");
    process.exit(1);
  }
  console.warn("denylist: RATHKIT_DENYLIST is not set, skipping (CI will fail without it)");
  process.exit(0);
}

if (process.argv.includes("--history")) {
  const hits = [];
  const git = (args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 30 });
  const lowerAll = (text) => text.toLowerCase().replace(/sha\d+-[a-z0-9+/=]+/g, "");
  // Messages and who wrote them.
  for (const record of git(["log", "--all", "--format=%H%x00%an%x00%ae%x00%cn%x00%B%x01"]).split("\x01")) {
    const [hash, ...rest] = record.trim().split("\x00");
    if (hash && denied.some((word) => lowerAll(rest.join("\n")).includes(word))) hits.push(`${hash.slice(0, 10)} (message or author)`);
  }
  // Every line any commit added, and every path any commit touched.
  let commit = "";
  let file = "";
  for (const line of git(["log", "--all", "-p", "-U0", "--no-color", "--no-renames", "--format=@@@%H"]).split("\n")) {
    if (line.startsWith("@@@")) { commit = line.slice(3, 13); continue; }
    if (line.startsWith("diff --git ")) {
      file = line.slice(line.indexOf(" b/") + 3);
      if (denied.some((word) => file.toLowerCase().includes(word))) hits.push(`${commit}:${file} (path)`);
      continue;
    }
    if (line.startsWith("+") && !line.startsWith("+++") && denied.some((word) => lowerAll(line).includes(word))) hits.push(`${commit}:${file}`);
  }
  const unique = [...new Set(hits)];
  if (unique.length > 0) {
    console.error(`denylist (history): ${unique.length} hit(s). Words are not printed; see commits and files:\n${unique.join("\n")}`);
    process.exit(1);
  }
  console.log("denylist (history): ok");
  process.exit(0);
}

const files = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
  encoding: "utf8",
})
  .split("\0")
  .filter(Boolean);

const hits = [];
for (const file of files) {
  let text;
  try {
    text = readFileSync(file, "utf8");
  } catch {
    continue;
  }
  if (text.includes("\0")) continue;
  const words = denied;
  // Integrity hashes are random base64, so short words match them by chance.
  const lower = [file.toLowerCase(), ...text.toLowerCase().split("\n").map((line) => line.replace(/sha\d+-[a-z0-9+/=]+/g, ""))];
  lower.forEach((line, index) => {
    if (words.some((word) => line.includes(word))) hits.push(index === 0 ? `${file} (path)` : `${file}:${index}`);
  });
}

if (hits.length > 0) {
  console.error(`denylist: ${hits.length} hit(s). Words are not printed; see locations:\n${hits.join("\n")}`);
  process.exit(1);
}
console.log("denylist: ok");
