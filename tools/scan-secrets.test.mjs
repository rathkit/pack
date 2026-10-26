import assert from "node:assert/strict";
import { test } from "node:test";
import { findings } from "./scan-secrets.mjs";

// The samples are built from pieces so that this file does not itself look like a secret.
const join = (...parts) => parts.join("");

test("the common secret formats are found", () => {
  assert.deepEqual(findings(join("-----BEGIN RSA ", "PRIVATE KEY-----")), ["private key"]);
  assert.deepEqual(findings(join("token ghp", "_", "a".repeat(36))), ["GitHub token"]);
  assert.deepEqual(findings(join("npm", "_", "b".repeat(36))), ["npm token"]);
  assert.deepEqual(findings(join("AKIA", "ABCDEFGHIJKLMNOP")), ["AWS access key"]);
  assert.deepEqual(findings(join('const apiKey = "', "x".repeat(20), '";')), ["password or key written into code"]);
  assert.deepEqual(findings(join("https://user:", "hunter22@example.test/path")), ["credentials in an address"]);
});

test("ordinary code, words and integrity hashes are not flagged", () => {
  for (const line of [
    "const token = readToken(options);",
    'const password = "";',
    "function secret(): string {",
    "see https://example.test/a/b?x=1",
    "integrity: sha512-" + "A1b2C3d4+/".repeat(12) + "==",
    'export const key = "Enter";',
  ]) assert.deepEqual(findings(line), [], line);
});
