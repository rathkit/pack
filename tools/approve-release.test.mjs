import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { crateFileName, isCode, npmNames, parseSums, pendingGate, splitStaged, stagedEntries, tarballName, waitingRun } from "./approve-release.mjs";

const hash = "a".repeat(64);
const V = "0.1.0-alpha.2";

test("checksum lines become a map by file name", () => {
  const sums = parseSums(`${hash}  rathkit-pack-0.1.0.tgz\n${"b".repeat(64)} *./x.crate\nnot a line\n`);
  assert.equal(sums.get("rathkit-pack-0.1.0.tgz"), hash);
  assert.equal(sums.get("x.crate"), "b".repeat(64));
  assert.equal(sums.size, 2);
});

test("tarball names drop the @ and turn / into -, and the package names are read back from SHA256SUMS", () => {
  assert.equal(tarballName("@rathkit/pack", V), `rathkit-pack-${V}.tgz`);
  assert.equal(tarballName("@rathkit/pack-linux-x64-musl", V), `rathkit-pack-linux-x64-musl-${V}.tgz`);
  assert.equal(crateFileName(V), `rathkit-pack-${V}.crate`);
  const sums = parseSums([`rathkit-pack-${V}.tgz`, `rathkit-pack-darwin-arm64-${V}.tgz`, `rathkit-pack-${V}-macos-arm64.tar.gz`, `rathkit-pack-${V}.crate`].map((f) => `${hash}  ${f}`).join("\n"));
  assert.deepEqual(npmNames(sums, V), ["@rathkit/pack", "@rathkit/pack-darwin-arm64"]);
  assert.deepEqual(npmNames(sums, "9.9.9"), []);
});

test("staged entries are read from a list or from an object holding one", () => {
  const one = { id: "s1", name: "@rathkit/pack", version: "1.0.0" };
  assert.deepEqual(stagedEntries([one]), [one]);
  assert.deepEqual(stagedEntries({ staged: [one] }), [one]);
  assert.deepEqual(stagedEntries({ error: "x" }), []);
  // the shape npm prints: packageName, and a status that must be "staged"
  const real = { id: "s1", packageName: "@rathkit/pack-win32-x64", version: V, tag: "next", status: "staged" };
  assert.deepEqual(stagedEntries([real, { ...real, id: "s2", status: "rejected" }]), [{ id: "s1", name: "@rathkit/pack-win32-x64", version: V }]);
  assert.deepEqual(stagedEntries([{ name: "a", version: "1" }]), []);
});

test("a package that is not staged is left to the caller, and a doubled one is a problem", () => {
  const e = [{ id: "1", name: "a", version: "1" }, { id: "2", name: "b", version: "1" }, { id: "3", name: "b", version: "1" }];
  assert.deepEqual(splitStaged(e, ["a"], "1"), { staged: [{ name: "a", id: "1" }], missing: [], problems: [] });
  assert.deepEqual(splitStaged(e, ["c"], "1").missing, ["c"]);
  assert.match(splitStaged(e, ["b"], "1").problems[0], /staged 2 times/);
  assert.deepEqual(splitStaged(e, ["a"], "2").missing, ["a"]);
});

test("the waiting run, the environment's gate and the code are picked correctly", () => {
  assert.equal(waitingRun([{ databaseId: 5, status: "completed" }, { databaseId: 7, status: "waiting" }, { databaseId: 9, status: "waiting" }]), 9);
  assert.equal(waitingRun([{ databaseId: 5, status: "completed" }]), undefined);
  const pending = [{ environment: { id: 1, name: "npm" }, current_user_can_approve: true }, { environment: { id: 2, name: "crates-io" }, current_user_can_approve: false }];
  assert.deepEqual(pendingGate(pending, "crates-io"), { environmentId: 2, canApprove: false });
  assert.equal(pendingGate(pending, "other"), undefined);
  assert.ok(isCode("123456") && !isCode("12345") && !isCode("") && !isCode("12345a"));
});

// The whole script against stand-in npm and gh programs on PATH (the real ones need the owner's login) and a stand-in crates.io.
const names = ["@rathkit/pack", ...["darwin-arm64", "darwin-x64", "linux-arm64-gnu", "linux-arm64-musl", "linux-x64-gnu", "linux-x64-musl", "win32-x64"].map((p) => `@rathkit/pack-${p}`)];
const sha = (text) => createHash("sha256").update(text).digest("hex");
const script = join(fileURLToPath(import.meta.url), "..", "approve-release.mjs");

const standIn = (body) => `#!${process.execPath}\nconst fs=require("fs"),path=require("path"),F=process.env.FAKE,a=process.argv.slice(2),J=(f)=>JSON.parse(fs.readFileSync(path.join(F,f),"utf8")),has=(f)=>fs.existsSync(path.join(F,f));\n${body}`;
const FAKE_NPM = `
if (a[0]==="stage"&&a[1]==="list") { console.log(JSON.stringify(J("stage.json"))); }
else if (a[0]==="view") { const n=a[1].replace(/@[^@]*$/,""); if (J("published.json").includes(n)) console.log(a[1].split("@").pop()); else process.exit(1); }
else if (a[0]==="stage"&&a[1]==="download") { fs.copyFileSync(path.join(F,"staged",a[2]+".tgz"), a[2]+".tgz"); }
else if (a[0]==="stage"&&a[1]==="approve") {
  const otp=a.includes("--otp")?a[a.indexOf("--otp")+1]:"";
  fs.appendFileSync(path.join(F,"approved.log"), a[2]+" otp="+otp+" browser="+process.env.BROWSER+"\\n");
  if (otp && otp!==fs.readFileSync(path.join(F,"valid-otp"),"utf8").trim()) { console.error("EOTP"); process.exit(1); }
}`;
const FAKE_GH = `
if (a[0]==="release") { const pat=a[a.indexOf("--pattern")+1], dir=a[a.indexOf("--dir")+1]; for (const f of fs.readdirSync(path.join(F,"release"))) if (f===pat) fs.copyFileSync(path.join(F,"release",f), path.join(dir,f)); if (!fs.existsSync(path.join(dir,pat))) process.exit(1); }
else if (a[0]==="run") { console.log(JSON.stringify(J("runs.json"))); }
else if (a[0]==="api"&&a.includes("POST")) { fs.writeFileSync(path.join(F,"posted.json"), fs.readFileSync(0,"utf8")); fs.writeFileSync(path.join(F,"posted-path"), a.join(" ")); }
else if (a[0]==="api") { console.log(JSON.stringify(J("pending.json"))); }`;

function world({ crateLive = false, tamper, published = [], staged, otp = "123456" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "approve-test-"));
  for (const d of ["bin", "release", "staged"]) mkdirSync(join(dir, d));
  for (const [file, body] of [["npm", FAKE_NPM], ["gh", FAKE_GH]]) { writeFileSync(join(dir, "bin", file), standIn(body)); chmodSync(join(dir, "bin", file), 0o755); }
  const lines = [];
  const stageList = [];
  names.forEach((name, i) => {
    const file = tarballName(name, V);
    lines.push(`${sha(file)}  ${file}`);
    writeFileSync(join(dir, "staged", `s${i}.tgz`), name === tamper ? "other bytes" : file);
    if (!published.includes(name)) stageList.push({ id: `s${i}`, packageName: name, version: V, tag: "next", status: "staged" });
  });
  lines.push(`${sha("the crate")}  ${crateFileName(V)}`);
  writeFileSync(join(dir, "release", crateFileName(V)), tamper === "crate" ? "other crate" : "the crate");
  writeFileSync(join(dir, "release", "SHA256SUMS"), lines.join("\n") + "\n");
  writeFileSync(join(dir, "stage.json"), JSON.stringify(staged ?? stageList));
  writeFileSync(join(dir, "published.json"), JSON.stringify(published));
  writeFileSync(join(dir, "valid-otp"), otp);
  writeFileSync(join(dir, "runs.json"), JSON.stringify([{ databaseId: 77, status: "waiting" }]));
  writeFileSync(join(dir, "pending.json"), JSON.stringify([{ environment: { id: 42, name: "crates-io" }, current_user_can_approve: true }]));
  return { dir, crateLive };
}

async function runScript(w, args, input = "") {
  const server = createServer((req, res) => { res.statusCode = w.crateLive ? 200 : 404; res.end("{}"); });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const env = { ...process.env, PATH: `${join(w.dir, "bin")}:${process.env.PATH}`, FAKE: w.dir, RKP_CRATES_API: `http://127.0.0.1:${server.address().port}` };
  delete env.BROWSER;
  const child = spawn(process.execPath, [script, `v${V}`, ...args], { env });
  let out = "";
  child.stdout.on("data", (d) => (out += d));
  child.stderr.on("data", (d) => (out += d));
  child.stdin.end(input);
  const code = await new Promise((r) => child.on("close", r));
  server.close();
  return { code, out };
}
const approvals = (w) => (existsSync(join(w.dir, "approved.log")) ? readFileSync(join(w.dir, "approved.log"), "utf8").trim().split("\n") : []);

test("without --approve it only checks", async () => {
  const w = world();
  const { code, out } = await runScript(w, []);
  assert.equal(code, 0, out);
  assert.match(out, /match +npm +@rathkit\/pack@/);
  assert.match(out, /match +crate/);
  assert.match(out, /nothing approved/);
  assert.equal(approvals(w).length, 0);
  assert.equal(existsSync(join(w.dir, "posted.json")), false);
});

test("one staged tarball that differs approves nothing", async () => {
  const w = world({ tamper: "@rathkit/pack-win32-x64" });
  const { code, out } = await runScript(w, ["--approve"], "yes\n123456\n");
  assert.equal(code, 1, out);
  assert.match(out, /MISMATCH +npm +@rathkit\/pack-win32-x64/);
  assert.equal(approvals(w).length, 0);
  assert.equal(existsSync(join(w.dir, "posted.json")), false);
});

test("a crate that differs approves nothing, npm included", async () => {
  const w = world({ tamper: "crate" });
  const { code, out } = await runScript(w, ["--approve"], "yes\n123456\n");
  assert.equal(code, 1, out);
  assert.match(out, /MISMATCH +crate/);
  assert.equal(approvals(w).length, 0);
});

test("--approve asks for one yes and one code, approves all eight with it, then the crates-io environment", async () => {
  const w = world();
  const { code, out } = await runScript(w, ["--approve"], "yes\n123456\n");
  assert.equal(code, 0, out);
  assert.equal(approvals(w).length, 8);
  assert.ok(approvals(w).every((l) => l.includes("otp=123456") && l.includes("browser=true")));
  const body = JSON.parse(readFileSync(join(w.dir, "posted.json"), "utf8"));
  assert.deepEqual([body.environment_ids, body.state], [[42], "approved"]);
  assert.match(readFileSync(join(w.dir, "posted-path"), "utf8"), /runs\/77\/pending_deployments/);
});

test("anything but yes approves nothing", async () => {
  const w = world();
  const { code } = await runScript(w, ["--approve"], "y\n");
  assert.equal(code, 0);
  assert.equal(approvals(w).length, 0);
  assert.equal(existsSync(join(w.dir, "posted.json")), false);
});

test("a package already published is skipped, and one that is neither staged nor published stops the run", async () => {
  const done = world({ published: ["@rathkit/pack-darwin-x64", "@rathkit/pack-win32-x64"] });
  const a = await runScript(done, ["--npm", "--approve"], "yes\n123456\n");
  assert.equal(a.code, 0, a.out);
  assert.match(a.out, /already published @rathkit\/pack-darwin-x64@/);
  assert.equal(approvals(done).length, 6);
  const lost = world({ staged: [] });
  const b = await runScript(lost, ["--npm"]);
  assert.equal(b.code, 1);
  assert.match(b.out, /is neither staged nor published/);
});

test("a code that expired is replaced and the package tried again", async () => {
  const w = world({ otp: "222222" });
  const { code, out } = await runScript(w, ["--npm", "--approve"], "yes\n111111\n222222\n");
  assert.equal(code, 0, out);
  const log = approvals(w);
  assert.equal(log.length, 9);
  assert.ok(log[0].includes("otp=111111") && log[1].includes("otp=222222"));
});

test("an empty code approves with no --otp (npm's browser sign-in)", async () => {
  const w = world();
  const { code, out } = await runScript(w, ["--npm", "--approve"], "yes\n\n");
  assert.equal(code, 0, out);
  assert.ok(approvals(w).every((l) => l.includes("otp= ")));
});

test("a crate already on crates.io is skipped; without a waiting run it is a problem", async () => {
  const live = world({ crateLive: true });
  const a = await runScript(live, ["--crates", "--approve"], "yes\n");
  assert.equal(a.code, 0, a.out);
  assert.match(a.out, /already published rathkit-pack@/);
  const idle = world();
  writeFileSync(join(idle.dir, "runs.json"), "[]");
  const b = await runScript(idle, ["--crates"]);
  assert.equal(b.code, 1);
  assert.match(b.out, /no run of the release workflow/);
});

test("a login that cannot approve the environment is a problem", async () => {
  const w = world();
  writeFileSync(join(w.dir, "pending.json"), JSON.stringify([{ environment: { id: 42, name: "crates-io" }, current_user_can_approve: false }]));
  const { code, out } = await runScript(w, ["--crates", "--approve"], "yes\n");
  assert.equal(code, 1);
  assert.match(out, /not a required reviewer/);
});
