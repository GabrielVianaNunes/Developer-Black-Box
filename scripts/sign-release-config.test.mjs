// Testes de onde a chave de assinatura é encontrada (signing.local.json, arquivo local fora do Git).
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { LOCAL_CONFIG, resolveKeyFile } from "./sign-release.mjs";
import { violations } from "./check-tracked.mjs";

const REPO = resolve(fileURLToPath(new URL("..", import.meta.url)));

function sandbox(config) {
  const root = mkdtempSync(join(tmpdir(), "bb-cfg-"));
  const keyFile = join(root, "outside.key");
  writeFileSync(keyFile, "synthetic");
  if (config !== undefined) writeFileSync(join(root, LOCAL_CONFIG), typeof config === "function" ? config(keyFile) : config);
  return { root, keyFile, done: () => rmSync(root, { recursive: true, force: true }) };
}

test("the key path comes from the local config, and --key-file overrides it", () => {
  const s = sandbox((key) => JSON.stringify({ keyFile: key }));
  try {
    assert.equal(resolveKeyFile({ root: s.root }), s.keyFile);
    assert.equal(resolveKeyFile({ flag: "explicit.key", root: s.root }), "explicit.key");
  } finally {
    s.done();
  }
});

test("a missing or broken config fails early with an actionable message", () => {
  const none = sandbox();
  const bad = sandbox("{ not json");
  const relative = sandbox(JSON.stringify({ keyFile: "relative/path.key" }));
  const nothing = sandbox(JSON.stringify({}));
  const gone = sandbox(() => JSON.stringify({ keyFile: join(tmpdir(), "bb-does-not-exist", "x.key") }));
  try {
    assert.throws(() => resolveKeyFile({ root: none.root }), /no signing key configured/);
    assert.throws(() => resolveKeyFile({ root: bad.root }), /not valid JSON/);
    assert.throws(() => resolveKeyFile({ root: relative.root }), /absolute path/);
    assert.throws(() => resolveKeyFile({ root: nothing.root }), /absolute path/);
    assert.throws(() => resolveKeyFile({ root: gone.root }), /does not exist/);
  } finally {
    for (const s of [none, bad, relative, nothing, gone]) s.done();
  }
});

test("a config that points inside the repository is refused", () => {
  const s = sandbox(JSON.stringify({ keyFile: join(REPO, "keys", "release.key") }));
  try {
    assert.throws(() => resolveKeyFile({ root: s.root }), /inside the repository/);
  } finally {
    s.done();
  }
});

test("the local config can never be committed", () => {
  assert.deepEqual(violations([LOCAL_CONFIG]).map((v) => v.rule), ["local-config"], "safety check rejects it if ever tracked");
  const ignored = spawnSync("git", ["check-ignore", "-q", LOCAL_CONFIG], { cwd: REPO });
  assert.equal(ignored.status, 0, ".gitignore must list signing.local.json");
  const tracked = execFileSync("git", ["ls-files"], { cwd: REPO, encoding: "utf8" }).split("\n");
  assert.ok(!tracked.includes(LOCAL_CONFIG), "signing.local.json must not be tracked");
});

test("no tracked file reveals where the private key lives or how to replace it", () => {
  const files = execFileSync("git", ["ls-files"], { cwd: REPO, encoding: "utf8" }).split("\n").filter((f) => /\.(md|mjs|rs|ts|tsx|yml|json)$/.test(f));
  const offenders = [];
  for (const f of files) {
    if (f.endsWith("sign-release-config.test.mjs")) continue;
    const text = execFileSync("git", ["show", `:${f}`], { cwd: REPO, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
    if (/developer-blackbox-signing|release-signing[.]key|lost or leaked/i.test(text)) offenders.push(f);
  }
  assert.deepEqual(offenders, []);
});
