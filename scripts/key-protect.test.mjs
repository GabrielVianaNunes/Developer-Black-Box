// Testes da proteção em repouso da chave de assinatura. Só chaves efêmeras e senhas de teste: nada real.
import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import {
  decryptWithPassphrase, encryptWithPassphrase, parseEnvelope, protectWithDpapi, readKeyFile, unprotectWithDpapi, wipeFile,
} from "./key-protect.mjs";
import { protectKeyFile, publicKeyHex, signRelease, sha256Hex } from "./sign-release.mjs";

const PASS = "correct horse battery staple";
const SCRIPT = fileURLToPath(new URL("./sign-release.mjs", import.meta.url));
const newKey = () => generateKeyPairSync("ed25519").privateKey;
const pemOf = (k) => k.export({ type: "pkcs8", format: "pem" });
const quiet = () => {};

function sandbox() {
  const dir = mkdtempSync(join(tmpdir(), "bb-keyprot-"));
  const key = newKey();
  const plain = join(dir, "plain.key");
  writeFileSync(plain, pemOf(key));
  return { dir, key, plain, done: () => rmSync(dir, { recursive: true, force: true }) };
}

test("passphrase: round trip, and nothing readable in the file", async () => {
  const pem = pemOf(newKey());
  const env = await encryptWithPassphrase(pem, PASS);
  assert.equal(env.kdf, "scrypt");
  assert.equal(await decryptWithPassphrase(env, PASS), pem);
  const text = JSON.stringify(env);
  assert.ok(!text.includes("PRIVATE KEY") && !text.includes(Buffer.from(pem).toString("base64").slice(0, 40)));
  const again = await encryptWithPassphrase(pem, PASS);
  assert.notEqual(again.salt, env.salt, "fresh salt every time");
  assert.notEqual(again.ct, env.ct, "fresh iv every time");
});

test("passphrase: wrong passphrase, tampering and absurd parameters are all refused", async () => {
  const env = await encryptWithPassphrase(pemOf(newKey()), PASS);
  await assert.rejects(decryptWithPassphrase(env, PASS + "x"), /wrong passphrase|altered/);
  await assert.rejects(decryptWithPassphrase(env, ""), /wrong passphrase|altered/);
  for (const field of ["ct", "tag", "salt", "iv"]) {
    const bad = { ...env, [field]: Buffer.from(env[field], "base64").map((b, i) => (i === 0 ? b ^ 1 : b)).toString("base64") };
    await assert.rejects(decryptWithPassphrase(bad, PASS), /wrong passphrase|altered|malformed/, field);
  }
  await assert.rejects(decryptWithPassphrase({ ...env, N: 2 ** 30 }, PASS), /unsupported key derivation/, "a file cannot ask for gigabytes");
  await assert.rejects(decryptWithPassphrase({ ...env, salt: "" }, PASS), /malformed/);
});

test("passphrase: a short passphrase is refused", async () => {
  await assert.rejects(encryptWithPassphrase("x", "short"), /at least 12/);
  await assert.rejects(encryptWithPassphrase("x", ""), /at least 12/);
});

test("envelope detection only accepts our own format", () => {
  assert.equal(parseEnvelope(pemOf(newKey())), null);
  assert.equal(parseEnvelope("not json"), null);
  assert.equal(parseEnvelope('{"v":2,"kdf":"scrypt"}'), null);
  assert.equal(parseEnvelope('{"v":1,"kdf":"rot13"}'), null);
  assert.equal(parseEnvelope("{ broken"), null);
  assert.ok(parseEnvelope('{"v":1,"kdf":"dpapi","data":"x"}'));
});

test("readKeyFile opens plain, passphrase-protected and DPAPI files", async () => {
  const s = sandbox();
  try {
    assert.equal(publicKeyHex(await readKeyFile(s.plain)), publicKeyHex(s.key));
    const enc = join(s.dir, "enc.key");
    writeFileSync(enc, JSON.stringify(await encryptWithPassphrase(pemOf(s.key), PASS)));
    assert.equal(publicKeyHex(await readKeyFile(enc, { passphrase: () => PASS })), publicKeyHex(s.key));
    await assert.rejects(readKeyFile(enc), /protected by a passphrase/);
    await assert.rejects(readKeyFile(enc, { passphrase: () => "wrong wrong wrong" }), /wrong passphrase/);
    if (process.platform === "win32") {
      const dp = join(s.dir, "dp.key");
      writeFileSync(dp, JSON.stringify(protectWithDpapi(pemOf(s.key))));
      assert.equal(publicKeyHex(await readKeyFile(dp)), publicKeyHex(s.key), "DPAPI needs no passphrase");
    }
  } finally {
    s.done();
  }
});

test("DPAPI: bound to this Windows account, nothing readable, tampering refused", { skip: process.platform !== "win32" }, () => {
  const pem = pemOf(newKey());
  const env = protectWithDpapi(pem);
  assert.equal(env.kdf, "dpapi");
  assert.ok(!JSON.stringify(env).includes("PRIVATE KEY"));
  assert.equal(unprotectWithDpapi(env), pem);
  const tampered = { ...env, data: Buffer.from(env.data, "base64").map((b, i) => (i === 40 ? b ^ 1 : b)).toString("base64") };
  assert.throws(() => unprotectWithDpapi(tampered), /DPAPI Unprotect failed/);
});

test("protect: writes a verified copy, then removes the original only when asked", async () => {
  const s = sandbox();
  try {
    const out = join(s.dir, "out.key");
    const r = await protectKeyFile({ input: s.plain, output: out, mode: "passphrase", passphrase: async () => PASS, log: quiet });
    assert.equal(r.publicKey, publicKeyHex(s.key));
    assert.ok(existsSync(s.plain), "the original stays without --remove-original");
    assert.equal(publicKeyHex(await readKeyFile(out, { passphrase: () => PASS })), publicKeyHex(s.key));

    const out2 = join(s.dir, "out2.key");
    await protectKeyFile({ input: s.plain, output: out2, mode: "passphrase", passphrase: async () => PASS, removeOriginal: true, log: quiet });
    assert.ok(!existsSync(s.plain), "the original is gone after a verified copy exists");
    assert.ok(existsSync(out2));
  } finally {
    s.done();
  }
});

test("protect: converts between forms (DPAPI -> passphrase) and never overwrites or writes inside the repo", async () => {
  const s = sandbox();
  try {
    await assert.rejects(protectKeyFile({ input: s.plain, output: "inside-repo.key", mode: "passphrase", passphrase: async () => PASS, log: quiet }), /inside the repository/);
    assert.ok(!existsSync("inside-repo.key"));
    const out = join(s.dir, "out.key");
    writeFileSync(out, "existing");
    await assert.rejects(protectKeyFile({ input: s.plain, output: out, mode: "passphrase", passphrase: async () => PASS, log: quiet }), /already exists/);
    assert.equal(readFileSync(out, "utf8"), "existing");
    if (process.platform === "win32") {
      const dp = join(s.dir, "dp.key");
      await protectKeyFile({ input: s.plain, output: dp, mode: "dpapi", log: quiet });
      const usb = join(s.dir, "usb.key");
      await protectKeyFile({ input: dp, output: usb, mode: "passphrase", passphrase: async () => PASS, log: quiet });
      assert.equal(publicKeyHex(await readKeyFile(usb, { passphrase: () => PASS })), publicKeyHex(s.key));
    }
  } finally {
    s.done();
  }
});

test("a short passphrase leaves nothing behind", async () => {
  const s = sandbox();
  try {
    const out = join(s.dir, "out.key");
    await assert.rejects(protectKeyFile({ input: s.plain, output: out, mode: "passphrase", passphrase: async () => "short", log: quiet }), /at least 12/);
    assert.ok(!existsSync(out));
    assert.ok(existsSync(s.plain));
  } finally {
    s.done();
  }
});

test("signing a release with a protected key: asks for the passphrase BEFORE touching GitHub", async () => {
  const s = sandbox();
  try {
    const enc = join(s.dir, "enc.key");
    writeFileSync(enc, JSON.stringify(await encryptWithPassphrase(pemOf(s.key), PASS)));
    const calls = [];
    const gh = (args) => {
      calls.push(args);
      throw new Error("GitHub must not be called before the key opens");
    };
    const previous = process.env.BB_KEY_PASSPHRASE;
    try {
      process.env.BB_KEY_PASSPHRASE = "wrong wrong wrong";
      await assert.rejects(signRelease({ tag: "v0.2.0", gh, keyFile: enc, confirm: async () => "yes", trusted: [publicKeyHex(s.key)], log: quiet }), /wrong passphrase/);
      assert.equal(calls.length, 0, "a wrong passphrase fails before any network call");
    } finally {
      if (previous === undefined) delete process.env.BB_KEY_PASSPHRASE;
      else process.env.BB_KEY_PASSPHRASE = previous;
    }
    // Com a senha certa o fluxo chega ao GitHub (aqui simulado para falhar logo depois).
    process.env.BB_KEY_PASSPHRASE = PASS;
    try {
      await assert.rejects(signRelease({ tag: "v0.2.0", gh, keyFile: enc, confirm: async () => "yes", trusted: [publicKeyHex(s.key)], log: quiet }), /GitHub must not be called/);
      assert.equal(calls.length, 1, "the right passphrase opens the key and the flow proceeds");
    } finally {
      delete process.env.BB_KEY_PASSPHRASE;
    }
  } finally {
    s.done();
  }
});

test("CLI check-key opens a protected key and compares it with the app's public key", async () => {
  const s = sandbox();
  try {
    const enc = join(s.dir, "enc.key");
    writeFileSync(enc, JSON.stringify(await encryptWithPassphrase(pemOf(s.key), PASS)));
    const run = (pass) => spawnSync(process.execPath, [SCRIPT, "check-key", "--key-file", enc], { env: { ...process.env, BB_KEY_PASSPHRASE: pass }, encoding: "utf8" });
    const bad = run("wrong wrong wrong");
    assert.notEqual(bad.status, 0);
    assert.match(bad.stderr, /wrong passphrase/);
    const good = run(PASS);
    assert.notEqual(good.status, 0, "an ephemeral test key is not the app's key");
    assert.match(good.stdout, /NOT among the app's trusted public keys/, "it opened, and was compared");
  } finally {
    s.done();
  }
});

test("wipeFile removes the file", () => {
  const s = sandbox();
  try {
    wipeFile(s.plain);
    assert.ok(!existsSync(s.plain));
    assert.equal(sha256Hex(Buffer.from("x")).length, 64);
  } finally {
    s.done();
  }
});
