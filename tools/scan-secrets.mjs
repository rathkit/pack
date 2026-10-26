// Looks for things that must never be committed: private keys, the common token formats, and passwords or keys written into code.
//   node tools/scan-secrets.mjs            the files now
//   node tools/scan-secrets.mjs --history  every line any commit on any branch or tag ever added
// What it finds is printed as a place and the kind of thing, never the value. It is a net, not a proof: anything it reports is looked at by
// a person, and a real secret is rotated, not just removed. (Lockfile integrity hashes are skipped, as they are random base64.)
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const PATTERNS = [
  ["private key", /-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY(?: BLOCK)?-----/],
  ["GitHub token", /\bgh[pousr]_[A-Za-z0-9]{36,}\b/],
  ["GitHub fine-grained token", /\bgithub_pat_[A-Za-z0-9_]{22,}\b/],
  ["npm token", /\bnpm_[A-Za-z0-9]{36}\b/],
  ["AWS access key", /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/],
  ["Slack token", /\bxox[abprs]-[A-Za-z0-9-]{10,}\b/],
  ["Google API key", /\bAIza[0-9A-Za-z_-]{35}\b/],
  ["Stripe key", /\b[sr]k_(?:live|test)_[0-9A-Za-z]{16,}\b/],
  ["JSON web token", /\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/],
  ["password or key written into code", /\b(?:secret|token|passw(?:or)?d|api[_-]?key|auth[_-]?key|private[_-]?key)\b["']?\s*[:=]\s*["'][^"'\s${}]{12,}["']/i],
  ["credentials in an address", /\b[a-z][a-z0-9+.-]*:\/\/[^\s/:@]+:[^\s/@]{3,}@[^\s/]+/i],
];
const SKIP_PATH = /(^|\/)(pnpm-lock\.yaml|package-lock\.json)$/;

/** The kinds of secret a line looks like (empty when none). */
export function findings(line) {
  const clean = line.replace(/sha\d+-[A-Za-z0-9+/=]+/g, "");
  return PATTERNS.filter(([, pattern]) => pattern.test(clean)).map(([name]) => name);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const hits = new Set();
  const git = (args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 30 });
  if (process.argv.includes("--history")) {
    let commit = "";
    let file = "";
    for (const line of git(["log", "--all", "-p", "-U0", "--no-color", "--no-renames", "--format=@@@%H"]).split("\n")) {
      if (line.startsWith("@@@")) { commit = line.slice(3, 13); continue; }
      if (line.startsWith("diff --git ")) { file = line.slice(line.indexOf(" b/") + 3); if (/(^|\/)\.env/.test(file)) hits.add(`${commit}:${file} (an .env file was committed)`); continue; }
      if (SKIP_PATH.test(file) || !line.startsWith("+") || line.startsWith("+++")) continue;
      for (const kind of findings(line.slice(1))) hits.add(`${commit}:${file} (${kind})`);
    }
  } else {
    for (const file of git(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).split("\0").filter(Boolean)) {
      if (SKIP_PATH.test(file)) continue;
      if (/(^|\/)\.env/.test(file)) hits.add(`${file} (an .env file)`);
      let text;
      try { text = readFileSync(file, "utf8"); } catch { continue; }
      if (text.includes("\0")) continue;
      text.split("\n").forEach((line, i) => { for (const kind of findings(line)) hits.add(`${file}:${i + 1} (${kind})`); });
    }
  }
  if (hits.size > 0) {
    console.error(`secrets: ${hits.size} place(s) to look at (the values are not printed):\n${[...hits].join("\n")}`);
    process.exit(1);
  }
  console.log(`secrets: nothing found${process.argv.includes("--history") ? " in any commit" : ""}`);
}
