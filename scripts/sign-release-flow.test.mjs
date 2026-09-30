// Testes do fluxo "assinar a Release localmente" (sem rede: o GitHub CLI é simulado).
// Chaves efêmeras; nenhuma chave real é usada.
import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { publicKeyHex, sha256Hex, signRelease, verifyFile } from "./sign-release.mjs";

const INSTALLER = "Developer-Black-Box_0.2.0_x64-setup.exe";
const BYTES = Buffer.from("synthetic installer bytes");

/** GitHub CLI simulado: guarda o que foi enviado e serve os arquivos da "Release". */
function fakeGh({ assets = [INSTALLER, "SHA256SUMS.txt"], installer = BYTES, sums } = {}) {
  const calls = [];
  const uploaded = [];
  const sumsText = sums ?? `${sha256Hex(installer)}  ${INSTALLER}\n`;
  const gh = (args) => {
    calls.push(args);
    if (args[1] === "view") return JSON.stringify({ isDraft: false, assets: assets.map((name) => ({ name })) });
    if (args[1] === "download") {
      const dir = args[args.indexOf("--dir") + 1];
      writeFileSync(join(dir, INSTALLER), installer);
      writeFileSync(join(dir, "SHA256SUMS.txt"), sumsText);
      return "";
    }
    if (args[1] === "upload") {
      const copy = join(mkdtempSync(join(tmpdir(), "bb-up-")), "uploaded.sig");
      copyFileSync(args[3], copy);
      uploaded.push({ tag: args[2], path: copy });
      return "";
    }
    throw new Error(`unexpected gh call: ${args.join(" ")}`);
  };
  return { gh, calls, uploaded };
}

function setup() {
  const dir = mkdtempSync(join(tmpdir(), "bb-flow-"));
  const key = generateKeyPairSync("ed25519").privateKey;
  const keyFile = join(dir, "k.key");
  writeFileSync(keyFile, key.export({ type: "pkcs8", format: "pem" }));
  return { dir, keyFile, trusted: [publicKeyHex(key)], done: () => rmSync(dir, { recursive: true, force: true }) };
}

const yes = async () => "yes";
const quiet = () => {};

test("downloads, checks the hash, signs locally and uploads ONLY the .sig", async () => {
  const s = setup();
  try {
    const f = fakeGh();
    const r = await signRelease({ tag: "v0.2.0", gh: f.gh, keyFile: s.keyFile, confirm: yes, trusted: s.trusted, log: quiet });
    assert.equal(r.sha, sha256Hex(BYTES));
    assert.equal(f.uploaded.length, 1);
    assert.equal(f.uploaded[0].tag, "v0.2.0");
    const sig = readFileSync(f.uploaded[0].path, "utf8").trim();
    assert.ok(verifyFile(s.trusted, "0.2.0", BYTES, sig), "the uploaded signature verifies for this exact installer and version");
    assert.ok(!verifyFile(s.trusted, "0.2.1", BYTES, sig), "and only for that version");
    const uploads = f.calls.filter((c) => c[1] === "upload");
    assert.equal(uploads.length, 1);
    assert.ok(uploads[0][3].endsWith(".sig"), "nothing but the signature is uploaded");
    assert.ok(!JSON.stringify(f.calls).includes("PRIVATE KEY"), "the private key never reaches the GitHub CLI");
  } finally {
    s.done();
  }
});

test("refuses to sign an installer that does not match SHA256SUMS.txt", async () => {
  const s = setup();
  try {
    const f = fakeGh({ sums: `${"0".repeat(64)}  ${INSTALLER}\n` });
    await assert.rejects(signRelease({ tag: "v0.2.0", gh: f.gh, keyFile: s.keyFile, confirm: yes, trusted: s.trusted, log: quiet }), /does not match SHA256SUMS/);
    assert.equal(f.uploaded.length, 0);
    const missing = fakeGh({ sums: "deadbeef  another-file.exe\n" });
    await assert.rejects(signRelease({ tag: "v0.2.0", gh: missing.gh, keyFile: s.keyFile, confirm: yes, trusted: s.trusted, log: quiet }), /does not match/);
  } finally {
    s.done();
  }
});

test("signs nothing unless the user types yes", async () => {
  const s = setup();
  try {
    for (const answer of ["", "no", "y", "sim", "YESS"]) {
      const f = fakeGh();
      await assert.rejects(signRelease({ tag: "v0.2.0", gh: f.gh, keyFile: s.keyFile, confirm: async () => answer, trusted: s.trusted, log: quiet }), /cancelled/, answer);
      assert.equal(f.uploaded.length, 0, `answer ${JSON.stringify(answer)}`);
    }
  } finally {
    s.done();
  }
});

test("refuses a key the app does not trust, without uploading", async () => {
  const s = setup();
  try {
    const f = fakeGh();
    const other = [publicKeyHex(generateKeyPairSync("ed25519").privateKey)];
    await assert.rejects(signRelease({ tag: "v0.2.0", gh: f.gh, keyFile: s.keyFile, confirm: yes, trusted: other, log: quiet }), /not among the app's trusted/);
    assert.equal(f.uploaded.length, 0);
  } finally {
    s.done();
  }
});

test("validates the tag and the release contents before touching anything", async () => {
  const s = setup();
  try {
    const args = { keyFile: s.keyFile, confirm: yes, trusted: s.trusted, log: quiet };
    for (const tag of ["0.2.0", "v0.2", "latest", "v", "v0.2.0; rm -rf /"]) {
      await assert.rejects(signRelease({ tag, gh: fakeGh().gh, ...args }), /invalid tag/, tag);
    }
    await assert.rejects(signRelease({ tag: "v0.2.0", gh: fakeGh({ assets: ["SHA256SUMS.txt"] }).gh, ...args }), /exactly one/);
    await assert.rejects(signRelease({ tag: "v0.2.0", gh: fakeGh({ assets: [INSTALLER, "b-setup.exe", "SHA256SUMS.txt"] }).gh, ...args }), /exactly one/);
    await assert.rejects(signRelease({ tag: "v0.2.0", gh: fakeGh({ assets: [INSTALLER] }).gh, ...args }), /no SHA256SUMS/);
  } finally {
    s.done();
  }
});

test("an already signed release is not re-signed unless asked", async () => {
  const s = setup();
  try {
    const signed = { assets: [INSTALLER, `${INSTALLER}.sig`, "SHA256SUMS.txt"] };
    const args = { tag: "v0.2.0", keyFile: s.keyFile, confirm: yes, trusted: s.trusted, log: quiet };
    const f = fakeGh(signed);
    await assert.rejects(signRelease({ gh: f.gh, ...args }), /already signed/);
    assert.equal(f.uploaded.length, 0);
    const g = fakeGh(signed);
    await signRelease({ gh: g.gh, ...args, resign: true });
    assert.equal(g.uploaded.length, 1);
  } finally {
    s.done();
  }
});
