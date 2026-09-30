// Testes dos scripts de segurança do repositório. Rode: npm test
// Os "segredos" são montados em tempo de execução para este arquivo não ser flagrado.
import assert from "node:assert/strict";
import { test } from "node:test";
import { violations } from "./check-tracked.mjs";
import { scanText } from "./scan-secrets.mjs";

const j = (...p) => p.join("");

test("check-tracked flags databases, recordings, logs, keys, env files and data folders", () => {
  const bad = [
    "meta.db", "data/meta.sqlite3", "seg-0000000001.bbseg", "recorder/journal.bbwal", "key.bin",
    "app.log", "crash.dmp", "server.pem", "id_rsa", ".env", ".env.local", "exports/incident-1.json",
    "evidence/x.txt", "config.local.json", "notes.bak",
  ];
  const got = new Set(violations(bad).map((v) => v.path));
  for (const p of bad) assert.ok(got.has(p), `should flag ${p}`);
});

test("check-tracked accepts normal project files and .env.example", () => {
  const ok = [
    "README.md", "src/app/App.tsx", "crates/bb-core/src/guard.rs", "tests/privacy/guard.rs",
    ".env.example", ".gitignore", "tests/privacy/repo.rs", "package.json", "Cargo.lock",
  ];
  assert.deepEqual(violations(ok), []);
});

test("scan-secrets finds private keys and well-known token formats", () => {
  const cases = [
    j("-----BEGIN RSA PRIV", "ATE KEY-----"),
    j("AK", "IA", "ABCDEFGHIJKLMNOP"),
    j("gh", "p_", "a".repeat(36)),
    j("xo", "xb-", "1234567890-abcdef"),
    j("eyJ", "hbGciOiJIUzI1NiJ9", ".eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0", ".abcdefghijklmnop"),
  ];
  for (const c of cases) assert.ok(scanText(c).length > 0, `should flag: ${c.slice(0, 12)}…`);
});

test("scan-secrets finds password and token assignments and URLs with credentials", () => {
  assert.ok(scanText(j("pass", 'word = "hunter2hunter2"')).length > 0);
  assert.ok(scanText(j("api_", 'key: "abcd1234efgh5678"')).length > 0);
  assert.ok(scanText(j("postgres://user:", "s3cretpw@localhost/db")).length > 0);
});

test("scan-secrets ignores synthetic markers and ordinary code", () => {
  assert.deepEqual(scanText(j("pass", 'word = "SYNTH-PASSWORD-0001"')), []);
  assert.deepEqual(scanText("let token_count = 3;\nconst name = 'notepad.exe';"), []);
  assert.deepEqual(scanText("// example: token = 'abcdefgh1234'"), []);
});

test("scan-secrets reports the line number", () => {
  const f = scanText(j("ok\nok\n", "AK", "IA", "ABCDEFGHIJKLMNOP\n"));
  assert.equal(f[0].line, 3);
});
