// Fails if the crate could pull third-party code in at build time.
// Rules, for the root crate and every other member of the workspace (the optional checker, roadmap 2.12): every dependency table in
// the manifest is empty, there is no build script, and Cargo.lock lists only the workspace's own packages, with no registry or git
// source and no dependency lines.
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const problems = [];

/** Checks the dependency tables of one manifest; returns its text. */
function checkManifest(dir, label) {
  const manifest = readFileSync(`${root}${dir}Cargo.toml`, "utf8");
  let section = "";
  for (const raw of manifest.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    const header = line.match(/^\[+([^\]]+)\]+$/);
    if (header) {
      section = header[1].trim();
      if (/(^|\.)(dev-|build-)?dependencies\./.test(section)) {
        problems.push(`${label}: [${section}] declares a dependency`);
      }
      continue;
    }
    if (/(^|\.)(dev-|build-)?dependencies$/.test(section)) {
      problems.push(`${label}: [${section}] must be empty, found "${line}"`);
    }
  }
  if (existsSync(`${root}${dir}build.rs`)) problems.push(`${dir}build.rs is not allowed`);
  return manifest;
}

const rootManifest = checkManifest("", "Cargo.toml");
const members = [];
const listed = rootManifest.match(/^\[workspace\][^[]*?members\s*=\s*\[([^\]]*)\]/ms);
for (const m of (listed?.[1] ?? "").matchAll(/"([^"]+)"/g)) if (m[1] !== ".") members.push(m[1].replace(/\/$/, ""));
const names = [rootManifest.match(/^name\s*=\s*"([^"]+)"/m)?.[1]];
for (const dir of members) {
  names.push(checkManifest(`${dir}/`, `${dir}/Cargo.toml`).match(/^name\s*=\s*"([^"]+)"/m)?.[1]);
}

if (existsSync(`${root}Cargo.lock`)) {
  const lock = readFileSync(`${root}Cargo.lock`, "utf8");
  const packages = [...lock.matchAll(/^name = "([^"]+)"/gm)].map((m) => m[1]);
  const extra = packages.filter((p) => !names.includes(p));
  if (extra.length > 0) problems.push(`Cargo.lock lists packages that are not workspace members: ${extra.join(", ")}`);
  if (/^source = /m.test(lock) || /^dependencies = /m.test(lock)) problems.push("Cargo.lock has a source or a dependency line");
}

if (problems.length > 0) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`zero-deps: ok (${names.join(", ")})`);
