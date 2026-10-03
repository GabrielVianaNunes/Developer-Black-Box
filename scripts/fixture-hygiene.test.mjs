// A higiene das fixtures: reprova o que parece dado real, aceita o sintético e vale para as fixtures de verdade.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { findProblems } from "./check-fixtures.mjs";

const rules = (text) => findProblems(text).map((p) => p.rule);

test("synthetic values pass", () => {
  const good = [
    "<Computer>SYNTH-PC</Computer>",
    "C:\\Users\\synth-user\\a.txt and /home/synth-user/b",
    "<Security UserID='S-1-5-21-0-0-0-1000'/> {00000000-0000-0000-0000-000000000000}",
    "version 1.0.0.0 and 3.14.2.1 are not network addresses",
    "SerialNumber SYNTH-SERIAL-0001",
  ].join("\n");
  assert.deepEqual(findProblems(good), []);
});

test("every kind of real-looking data is rejected", () => {
  const bad = {
    "computer-name": "<Computer>WORKSTATION-01</Computer>",
    "user-folder": "C:\\Users\\maria\\Documents\\a.txt",
    email: "contact: someone@company.com",
    "mac-address": "adapter 00:1A:2B:3C:4D:5E",
    guid: "{5f3a1b2c-1111-2222-3333-444455556666}",
    sid: "S-1-5-21-1234567890-1234567890-1234567890-1001",
    "private-ip": "gateway 192.168.1.1",
    "default-computer-name": "host DESKTOP-AB12CD3",
    "serial-number": "SerialNumber: PF2ABCDE",
  };
  for (const [rule, text] of Object.entries(bad)) assert.ok(rules(text).includes(rule), `${rule} must be caught in: ${text}`);
});

test("other private-range and unix-style user paths are caught too", () => {
  assert.ok(rules("10.0.0.7").includes("private-ip"));
  assert.ok(rules("172.20.4.4").includes("private-ip"));
  assert.ok(rules("/home/john/file").includes("user-folder"));
  assert.ok(rules("/Users/john/file").includes("user-folder"));
  assert.deepEqual(rules("172.40.4.4"), [], "outside the private range");
});

test("a non-zero GUID is caught even if only one digit differs", () => {
  assert.ok(rules("{00000000-0000-0000-0000-000000000001}").includes("guid"));
});

function walk(dir) {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

test("every real fixture in the repository passes", () => {
  const root = fileURLToPath(new URL("../tests/fixtures", import.meta.url));
  const files = walk(root);
  assert.ok(files.length >= 30, `positive control: the fixtures exist (${files.length})`);
  for (const f of files) assert.deepEqual(findProblems(readFileSync(f, "utf8")), [], f);
});

test("the checker script itself succeeds on the repository", () => {
  const out = execFileSync("node", ["scripts/check-fixtures.mjs"], { encoding: "utf8", cwd: fileURLToPath(new URL("..", import.meta.url)) });
  assert.match(out, /OK: \d+ fixture file/);
});
