// Checks what is about to go live for a release against the release's signed checksums and, only when asked, approves it:
//   node tools/approve-release.mjs v0.1.0-alpha.2             (check only; nothing is approved)
//   node tools/approve-release.mjs v0.1.0-alpha.2 --approve   (check, ask once, then approve; npm asks for the two-factor code)
//   --npm or --crates limits it to one registry (the default is both).
// Needs `npm login` as the owner for npm, and `gh` logged in as the owner (it downloads the release's SHA256SUMS and approves the
// crates-io environment). Approving publishes: it cannot be undone, and a version number can never be reused.
//
// The two registries differ, and the script follows each one's own shape:
//
//   npm has a staging step. The `npm` job stages the eight packages (`npm stage publish`, with provenance) and nothing is live until
//   the owner approves each with a two-factor code. So the script downloads every staged tarball (`npm stage download <id>`), compares
//   its sha256 with the release's SHA256SUMS, and only if all eight match runs `npm stage approve <id> --otp <code>`.
//
//   crates.io has no staging. `cargo publish` goes live at once, and a version can be yanked but never removed or replaced, so the
//   check has to come before the publish, and the gate is the `crates-io` GitHub environment's required reviewer. The `crates` job
//   waits at that gate before it runs anything. So before the owner approves, the script downloads the release's .crate and compares
//   its sha256 with SHA256SUMS, checks that the version is not already on crates.io, and finds the run waiting at the gate. With
//   --approve it approves that environment (the API behind the "Review deployments" button). The byte-for-byte check of what is
//   published is made again in the job itself, after approval and before the token is even requested: it packs the crate again
//   (cargo packs reproducibly) and stops unless the hash is the one in SHA256SUMS. Trusted publishing (OIDC) gives that job a
//   30-minute token, so no long-lived token is stored anywhere; crates.io offers it for GitHub Actions and GitLab only, and a token
//   it gives can publish but not yank.
//
// A run can stop part way. A package or crate that is no longer waiting is fine when its version is already live (npm view / the
// crates.io API say so): it is skipped. The script stops only when something is neither waiting nor live, or is staged twice.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

export const REPO = "rathkit/pack";
export const CRATE = "rathkit-pack";
export const CRATES_ENVIRONMENT = "crates-io";
/** The launcher and one package per platform. */
export const EXPECTED_NPM_PACKAGES = 8;

/** `sha256  file` lines to a Map of file name to hash. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split("\n")) {
    const m = /^([0-9a-f]{64})\s+\*?(.+)$/.exec(line.trim());
    if (m) sums.set(m[2].replace(/^.*\//, ""), m[1]);
  }
  return sums;
}

/** The file name of an npm package's tarball: the scope's @ dropped and / turned into -. */
export const tarballName = (name, version) => `${name.replace(/^@/, "").replace("/", "-")}-${version}.tgz`;

export const crateFileName = (version) => `${CRATE}-${version}.crate`;

/** The npm packages a release holds, read from the .tgz names in SHA256SUMS (rathkit-pack-<platform>-<version>.tgz is @rathkit/pack-<platform>). */
export function npmNames(sums, version) {
  const suffix = `-${version}.tgz`;
  return [...sums.keys()]
    .filter((f) => f.startsWith("rathkit-") && f.endsWith(suffix))
    .map((f) => `@rathkit/${f.slice("rathkit-".length, -suffix.length)}`)
    .sort();
}

/** The staged entries `npm stage list --json` printed, as { id, name, version }; only entries whose status is "staged" count. */
export function stagedEntries(json) {
  const list = Array.isArray(json) ? json : Object.values(json ?? {}).find(Array.isArray) ?? [];
  return list
    .filter((e) => e.status === undefined || e.status === "staged")
    .map((e) => ({ id: e.id ?? e.stageId ?? e.stage_id, name: e.packageName ?? e.name ?? e.package, version: e.version }))
    .filter((e) => e.id && e.name && e.version);
}

/**
 * Splits the package names into those staged for `version` and the rest, and lists what is wrong (empty when fine):
 * a name staged more than once is a problem; a name not staged is left to the caller, which checks whether it is already published.
 */
export function splitStaged(entries, names, version) {
  const staged = [];
  const missing = [];
  const problems = [];
  for (const name of names) {
    const found = entries.filter((e) => e.name === name && e.version === version);
    if (found.length === 0) missing.push(name);
    else if (found.length > 1) problems.push(`${name}@${version} is staged ${found.length} times (${found.map((e) => e.id).join(", ")}); reject the extra ones by hand`);
    else staged.push({ name, id: found[0].id });
  }
  return { staged, missing, problems };
}

/** The newest run of the release workflow that is waiting for an approval, from `gh run list --json databaseId,status`. */
export function waitingRun(runs) {
  return (runs ?? []).filter((r) => r.status === "waiting").map((r) => r.databaseId).sort((a, b) => b - a)[0];
}

/** The pending deployment of the named environment, from the API's list: { environmentId, canApprove }. */
export function pendingGate(pending, environmentName) {
  const found = (pending ?? []).find((p) => p.environment?.name === environmentName);
  return found ? { environmentId: found.environment.id, canApprove: found.current_user_can_approve === true } : undefined;
}

/** What the owner types is a six-digit code, or nothing (npm then opens the browser for each package). */
export const isCode = (text) => /^\d{6}$/.test(text);

const sha256 = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
const run = (cmd, args, options = {}) => spawnSync(cmd, args, { encoding: "utf8", ...options });

/** Asks questions on stdin one line at a time; lines that arrive before a question is asked are kept (piped input). */
function lineReader() {
  const rl = createInterface({ input: process.stdin });
  const lines = [];
  const waiting = [];
  let closed = false;
  rl.on("line", (l) => (waiting.length ? waiting.shift()(l) : lines.push(l)));
  rl.on("close", () => { closed = true; for (const w of waiting.splice(0)) w(""); });
  return {
    ask(prompt) {
      process.stdout.write(prompt);
      if (lines.length) return Promise.resolve(lines.shift());
      if (closed) return Promise.resolve("");
      return new Promise((resolve) => waiting.push(resolve));
    },
    close: () => rl.close(),
  };
}

/** Is `version` of the crate on crates.io? True, false, or a thrown error when the registry cannot be asked. */
async function crateIsLive(version) {
  const base = process.env.RKP_CRATES_API ?? "https://crates.io/api/v1/crates";
  const res = await fetch(`${base}/${CRATE}/${version}`, { headers: { "User-Agent": "rathkit-pack approve-release (https://github.com/rathkit/pack)" } });
  if (res.status === 200) return true;
  if (res.status === 404) return false;
  throw new Error(`crates.io answered ${res.status} when asked for ${CRATE}@${version}`);
}

/** Checks the staged npm packages; returns { chosen, published, problems, bad }. Approves nothing. */
function checkNpm(version, sums, work) {
  const result = { chosen: [], published: [], problems: [], bad: [] };
  const names = npmNames(sums, version);
  if (names.length !== EXPECTED_NPM_PACKAGES) {
    result.problems.push(`SHA256SUMS lists ${names.length} npm packages for ${version}, not ${EXPECTED_NPM_PACKAGES}`);
    return result;
  }
  const listed = run("npm", ["stage", "list", "--json"]);
  if (listed.status !== 0) { result.problems.push(`npm stage list failed: ${(listed.stderr || listed.stdout).trim()}`); return result; }
  let json;
  try { json = JSON.parse(listed.stdout); } catch { result.problems.push("npm stage list did not print JSON"); return result; }
  const { staged, missing, problems } = splitStaged(stagedEntries(json), names, version);
  result.problems.push(...problems);
  // A package that is not staged is fine only if that version is already on npm (a run that stopped half way).
  for (const name of missing) {
    const live = run("npm", ["view", `${name}@${version}`, "version"]);
    if (live.status === 0 && live.stdout.trim() === version) result.published.push(name);
    else result.problems.push(`${name}@${version} is neither staged nor published`);
  }
  if (result.problems.length) return result;
  for (const { name, id } of staged) {
    const dir = mkdtempSync(join(work, "dl-"));
    const got = run("npm", ["stage", "download", id], { cwd: dir });
    const file = readdirSync(dir).find((f) => f.endsWith(".tgz"));
    const want = sums.get(tarballName(name, version));
    const have = got.status === 0 && file ? sha256(join(dir, file)) : undefined;
    const ok = want !== undefined && have === want;
    console.log(`${ok ? "match   " : "MISMATCH"} npm   ${name}@${version}  ${id}`);
    if (!ok) result.bad.push(name);
  }
  result.chosen = staged;
  return result;
}

/** Checks the crate in the release and the gate in front of crates.io; returns { live, gate, runId, problems, bad }. Approves nothing. */
async function checkCrate(tag, version, sums, work) {
  const result = { live: false, gate: undefined, runId: undefined, problems: [], bad: [] };
  const file = crateFileName(version);
  result.live = await crateIsLive(version);
  if (result.live) return result;
  const want = sums.get(file);
  if (!want) { result.problems.push(`${file} is not in SHA256SUMS`); return result; }
  const got = run("gh", ["release", "download", tag, "--repo", REPO, "--pattern", file, "--dir", work]);
  const have = got.status === 0 ? sha256(join(work, file)) : undefined;
  const ok = have === want;
  console.log(`${ok ? "match   " : "MISMATCH"} crate ${CRATE}@${version}`);
  if (!ok) result.bad.push(CRATE);
  const runs = run("gh", ["run", "list", "--repo", REPO, "--workflow", "release.yml", "--event", "push", "--branch", tag, "--json", "databaseId,status", "--limit", "10"]);
  result.runId = runs.status === 0 ? waitingRun(JSON.parse(runs.stdout || "[]")) : undefined;
  if (result.runId === undefined) { result.problems.push(`${CRATE}@${version} is not on crates.io, and no run of the release workflow for ${tag} is waiting for approval`); return result; }
  const pending = run("gh", ["api", `repos/${REPO}/actions/runs/${result.runId}/pending_deployments`]);
  result.gate = pending.status === 0 ? pendingGate(JSON.parse(pending.stdout || "[]"), CRATES_ENVIRONMENT) : undefined;
  if (!result.gate) result.problems.push(`run ${result.runId} is not waiting at the ${CRATES_ENVIRONMENT} environment`);
  else if (!result.gate.canApprove) result.problems.push(`this gh login is not a required reviewer of the ${CRATES_ENVIRONMENT} environment`);
  return result;
}

async function main() {
  const [tag, ...flags] = process.argv.slice(2);
  if (!tag || !/^v\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?$/.test(tag)) {
    console.error("usage: node tools/approve-release.mjs v<version> [--npm] [--crates] [--approve]");
    process.exit(2);
  }
  const approve = flags.includes("--approve");
  const both = !flags.includes("--npm") && !flags.includes("--crates");
  const doNpm = both || flags.includes("--npm");
  const doCrates = both || flags.includes("--crates");
  const version = tag.slice(1);
  const work = mkdtempSync(join(tmpdir(), "approve-"));

  const got = run("gh", ["release", "download", tag, "--repo", REPO, "--pattern", "SHA256SUMS", "--dir", work], { stdio: ["ignore", "inherit", "inherit"] });
  if (got.status !== 0) { console.error("could not download SHA256SUMS of the release"); process.exit(1); }
  const sums = parseSums(readFileSync(join(work, "SHA256SUMS"), "utf8"));

  // Check everything first, and only then approve anything, so a bad artifact is caught before anything goes live.
  const npm = doNpm ? checkNpm(version, sums, work) : undefined;
  const crate = doCrates ? await checkCrate(tag, version, sums, work) : undefined;
  const problems = [...(npm?.problems ?? []), ...(crate?.problems ?? [])];
  const bad = [...(npm?.bad ?? []), ...(crate?.bad ?? [])];
  if (problems.length) { for (const p of problems) console.error(`  ${p}`); process.exit(1); }
  for (const name of npm?.published ?? []) console.log(`already published ${name}@${version}`);
  if (crate?.live) console.log(`already published ${CRATE}@${version} on crates.io`);
  if (bad.length) { console.error(`\nnot approving: ${bad.length} file(s) differ from SHA256SUMS or could not be checked: ${bad.join(", ")}`); process.exit(1); }
  const npmCount = npm?.chosen.length ?? 0;
  const crateWaiting = crate && !crate.live;
  if (npmCount === 0 && !crateWaiting) { console.log("nothing is waiting: everything asked about is already published."); return; }
  console.log(`\nall ${npmCount ? `${npmCount} staged npm tarballs` : ""}${npmCount && crateWaiting ? " and " : ""}${crateWaiting ? "the crate" : ""} match the release's SHA256SUMS.`);
  if (!approve) { console.log("nothing approved. Run again with --approve to publish."); return; }

  const input = lineReader();
  const what = [npmCount ? `${npmCount} npm packages` : "", crateWaiting ? `the crate on crates.io` : ""].filter(Boolean).join(" and ");
  const answer = await input.ask(`Approve ${what} at ${version}? This publishes them and cannot be undone. Type "yes": `);
  if (answer.trim() !== "yes") { input.close(); console.log("nothing approved."); return; }

  if (npmCount) {
    // One code from the authenticator app serves every package while it is valid (about half a minute), so ask once and pass it to each
    // approval. With no code (an account that signs in by browser or security key) npm asks for each package on its own.
    let otp = (await input.ask("\nCode from your authenticator app (6 digits; leave empty to approve in the browser for each package): ")).trim();
    if (otp !== "" && !isCode(otp)) { input.close(); console.error("that is not a 6-digit code; nothing approved."); process.exit(1); }
    for (const { name, id } of npm.chosen) {
      for (;;) {
        console.log(`\napproving ${name}@${version}`);
        // npm says "Press ENTER to open in the browser" and fails if no browser is set (WSL has none). With BROWSER set to a command
        // that does nothing, a stray Enter is harmless and npm keeps waiting for the approval opened by hand.
        const r = spawnSync("npm", ["stage", "approve", id, ...(otp ? ["--otp", otp] : [])], { stdio: "inherit", env: { BROWSER: "true", ...process.env } });
        if (r.status === 0) break;
        const again = otp ? (await input.ask("That failed; the code may have expired. Enter a fresh 6-digit code to retry this package (empty to stop): ")).trim() : "";
        if (!isCode(again)) {
          input.close();
          console.error(`stopped: approving ${name} failed; the ones before it are live, the rest are still staged. Run it again: it skips the ones already published.`);
          process.exit(1);
        }
        otp = again;
      }
    }
    console.log("\nnpm done. Check: npm view @rathkit/pack dist-tags");
  }
  input.close();

  if (crateWaiting) {
    console.log(`\napproving the ${CRATES_ENVIRONMENT} environment of run ${crate.runId}`);
    const body = JSON.stringify({ environment_ids: [crate.gate.environmentId], state: "approved", comment: `${CRATE}@${version} matches SHA256SUMS (tools/approve-release.mjs)` });
    const r = run("gh", ["api", "-X", "POST", `repos/${REPO}/actions/runs/${crate.runId}/pending_deployments`, "--input", "-"], { input: body });
    if (r.status !== 0) { console.error(r.stderr || r.stdout); process.exit(1); }
    console.log(`approved. The crates job now packs the crate again, checks it against SHA256SUMS and publishes. Check: cargo info ${CRATE}@${version}`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
