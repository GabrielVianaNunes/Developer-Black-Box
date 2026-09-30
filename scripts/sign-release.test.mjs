// Testes da assinatura das Releases. Rode: npm test
// Todas as chaves aqui são geradas na hora e descartadas; nenhuma chave real é usada.
import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { assertOutsideRepo, publicKeyHex, sha256Hex, signFile, signedMessage, trustedKeys, verifyFile } from "./sign-release.mjs";

const SCRIPT = fileURLToPath(new URL("./sign-release.mjs", import.meta.url));
const FILE = Buffer.from("synthetic installer bytes");
const ephemeral = () => generateKeyPairSync("ed25519").privateKey;

test("a signature verifies for the same file and version, and only for them", () => {
  const key = ephemeral();
  const trusted = [publicKeyHex(key)];
  const sig = signFile(key, "0.2.0", FILE);
  assert.ok(verifyFile(trusted, "0.2.0", FILE, sig));
  assert.ok(!verifyFile(trusted, "0.2.0", Buffer.from("synthetic installer bytez"), sig), "tampered file");
  assert.ok(!verifyFile(trusted, "0.2.1", FILE, sig), "an old installer cannot pose as a newer version");
  assert.ok(!verifyFile([publicKeyHex(ephemeral())], "0.2.0", FILE, sig), "untrusted key");
  assert.ok(!verifyFile([], "0.2.0", FILE, sig), "no trusted key accepts nothing");
  for (const bad of ["", "zz", sig.slice(1), sig + "0", sig.toUpperCase(), ` ${sig}`]) {
    assert.ok(!verifyFile(trusted, "0.2.0", FILE, bad), `malformed: ${bad.slice(0, 8)}`);
  }
});

test("any trusted key is enough (key rotation)", () => {
  const [a, b] = [ephemeral(), ephemeral()];
  assert.ok(verifyFile([publicKeyHex(a), publicKeyHex(b)], "1.0.0", FILE, signFile(b, "1.0.0", FILE)));
});

test("the signed text binds version and hash with a fixed prefix", () => {
  const sha = sha256Hex(FILE);
  assert.equal(signedMessage("0.2.0", sha).toString(), `DeveloperBlackBox-release-v1\n0.2.0\n${sha}\n`);
  assert.throws(() => signedMessage("v0.2.0", sha));
  assert.throws(() => signedMessage("0.2.0", "abc"));
});

test("the app's trusted keys are read from verify.rs and are well formed", () => {
  const keys = trustedKeys();
  assert.ok(keys.length >= 1 && keys.every((k) => /^[0-9a-f]{64}$/.test(k)));
  assert.throws(() => trustedKeys("pub const OTHER: u8 = 1;"), /no trusted public key/);
});

test("the committed cross-language vector verifies with the script's own code", () => {
  const v = JSON.parse(readFileSync(fileURLToPath(new URL("../tests/privacy/release_signature_vector.json", import.meta.url)), "utf8"));
  assert.ok(verifyFile([v.publicKey], v.version, Buffer.from(v.fileUtf8), v.signature));
  assert.ok(!trustedKeys().includes(v.publicKey), "the test vector key must never be a trusted release key");
});

test("a private key can never be written inside the repository", () => {
  assert.throws(() => assertOutsideRepo("release.key"), /inside the repository/);
  assert.throws(() => assertOutsideRepo("scripts/../keys/x.pem"), /inside the repository/);
  assert.doesNotThrow(() => assertOutsideRepo(join(tmpdir(), "outside.key")));
});

test("CLI: signing with a key the app does not trust is refused and writes no .sig", () => {
  const dir = mkdtempSync(join(tmpdir(), "bb-sign-"));
  try {
    const installer = join(dir, "setup.exe");
    writeFileSync(installer, FILE);
    const pem = ephemeral().export({ type: "pkcs8", format: "pem" });
    const r = spawnSync(process.execPath, [SCRIPT, "sign", installer, "0.2.0"], { env: { ...process.env, BB_SIGNING_KEY: pem }, encoding: "utf8" });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /not among the app's trusted public keys/);
    assert.ok(!existsSync(`${installer}.sig`));
    const none = spawnSync(process.execPath, [SCRIPT, "sign", installer, "0.2.0"], { env: { ...process.env, BB_SIGNING_KEY: "" }, encoding: "utf8" });
    assert.notEqual(none.status, 0);
    assert.match(none.stderr, /no signing key/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("CLI: keygen refuses the repository and never prints the private key", () => {
  const inside = spawnSync(process.execPath, [SCRIPT, "keygen", "leak.key"], { encoding: "utf8" });
  assert.notEqual(inside.status, 0);
  assert.ok(!existsSync("leak.key"));
  const dir = mkdtempSync(join(tmpdir(), "bb-keygen-"));
  try {
    const out = join(dir, "k.key");
    const ok = spawnSync(process.execPath, [SCRIPT, "keygen", out], { encoding: "utf8" });
    assert.equal(ok.status, 0, ok.stderr);
    assert.ok(!ok.stdout.includes("PRIVATE KEY"));
    assert.ok(readFileSync(out, "utf8").includes("BEGIN PRIVATE KEY"));
    assert.ok(ok.stdout.includes(publicKeyHex(readFileSync(out, "utf8"))));
    assert.notEqual(spawnSync(process.execPath, [SCRIPT, "keygen", out], { encoding: "utf8" }).status, 0, "no overwrite");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
