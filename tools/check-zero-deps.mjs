// Fails if the crate could pull third-party code in at build time.
// Rules: every dependency table in Cargo.toml is empty, there is no build script,
// and Cargo.lock lists only this crate.
import { existsSync, readFileSync } from "node:fs";

const root = new URL("..", import.meta.url).pathname;
const problems = [];

const manifest = readFileSync(`${root}Cargo.toml`, "utf8");
let section = "";
for (const raw of manifest.split("\n")) {
  const line = raw.trim();
  if (line === "" || line.startsWith("#")) continue;
  const header = line.match(/^\[+([^\]]+)\]+$/);
  if (header) {
    section = header[1].trim();
    if (/(^|\.)(dev-|build-)?dependencies\./.test(section)) {
      problems.push(`Cargo.toml: [${section}] declares a dependency`);
    }
    continue;
  }
  if (/(^|\.)(dev-|build-)?dependencies$/.test(section)) {
    problems.push(`Cargo.toml: [${section}] must be empty, found "${line}"`);
  }
}

if (existsSync(`${root}build.rs`)) problems.push("build.rs is not allowed");

if (existsSync(`${root}Cargo.lock`)) {
  const count = (readFileSync(`${root}Cargo.lock`, "utf8").match(/^\[\[package\]\]/gm) ?? []).length;
  if (count > 1) problems.push(`Cargo.lock lists ${count} packages, expected only this crate`);
}

if (problems.length > 0) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log("zero-deps: ok");
