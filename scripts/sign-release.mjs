// Assinatura das Releases (Ed25519). O app só instala uma atualização cuja assinatura confere com uma
// chave pública embutida nele (crates/bb-update/src/verify.rs), então só quem tem a chave privada
// consegue publicar uma atualização que os apps aceitam.
//
// Uso: node scripts/sign-release.mjs keygen <arquivo-da-chave-privada>   gera o par; a privada NUNCA fica no repositório
//      node scripts/sign-release.mjs sign <instalador> <versão>          grava <instalador>.sig (chave em BB_SIGNING_KEY ou --key-file)
//      node scripts/sign-release.mjs verify <instalador> <versão>        confere o .sig com as chaves embutidas no app
//      node scripts/sign-release.mjs release <tag>                       baixa o instalador da Release, confere o SHA-256, assina AQUI e envia só o .sig
//
// A chave privada nunca sai do seu computador: o CI só constrói e publica o instalador; quem assina é você.
//
// A assinatura cobre "DeveloperBlackBox-release-v1\n<versão>\n<sha256 do instalador>\n": amarra o arquivo à
// versão, então um instalador antigo (assinado) não pode ser apresentado como uma versão mais nova.
import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { createInterface } from "node:readline/promises";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isVersion } from "./version.mjs";

export const MESSAGE_PREFIX = "DeveloperBlackBox-release-v1";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const VERIFY_RS = "crates/bb-update/src/verify.rs";

export const sha256Hex = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function signedMessage(version, sha256) {
  if (!isVersion(version)) throw new Error(`invalid version: ${version}`);
  if (!/^[0-9a-f]{64}$/.test(sha256)) throw new Error("invalid sha256");
  return Buffer.from(`${MESSAGE_PREFIX}\n${version}\n${sha256}\n`, "utf8");
}

/** Chave pública (32 bytes, em hex) de uma chave privada ou pública. */
export function publicKeyHex(key) {
  const der = createPublicKey(key).export({ type: "spki", format: "der" });
  return der.subarray(der.length - 32).toString("hex"); // SPKI Ed25519: 12 bytes de cabeçalho + 32 da chave
}

function publicKeyFromHex(hex) {
  if (!/^[0-9a-f]{64}$/.test(hex)) throw new Error("invalid public key");
  const header = Buffer.from("302a300506032b6570032100", "hex");
  return createPublicKey({ key: Buffer.concat([header, Buffer.from(hex, "hex")]), format: "der", type: "spki" });
}

export function signFile(privateKey, version, fileBytes) {
  return sign(null, signedMessage(version, sha256Hex(fileBytes)), privateKey).toString("hex");
}

/** true se `signatureHex` confere com alguma das chaves públicas (hex). */
export function verifyFile(trustedKeysHex, version, fileBytes, signatureHex) {
  if (!/^[0-9a-f]{128}$/.test(signatureHex)) return false;
  const message = signedMessage(version, sha256Hex(fileBytes));
  return trustedKeysHex.some((k) => verify(null, message, publicKeyFromHex(k), Buffer.from(signatureHex, "hex")));
}

/** Chaves públicas de confiança embutidas no app (fonte: o próprio verify.rs, para nunca divergirem). */
export function trustedKeys(rustSource = readFileSync(resolve(ROOT, VERIFY_RS), "utf8")) {
  const block = /TRUSTED_PUBLIC_KEYS:\s*\[&str;\s*\d+\]\s*=\s*\[([^\]]*)\]/.exec(rustSource)?.[1];
  const keys = [...(block ?? "").matchAll(/"([0-9a-f]{64})"/g)].map((m) => m[1]);
  if (keys.length === 0) throw new Error(`no trusted public key found in ${VERIFY_RS}`);
  return keys;
}

/** A chave privada nunca pode ficar dentro do repositório. */
export function assertOutsideRepo(file) {
  const rel = relative(ROOT, resolve(file));
  if (!rel.startsWith("..") && !isAbsolute(rel)) throw new Error(`refusing to put a private key inside the repository (${rel})`);
}

export const DEFAULT_KEY_FILE = join(homedir(), ".developer-blackbox-signing", "release-signing.key");

/**
 * Assina uma Release já publicada pelo CI: baixa o instalador, confere com o SHA256SUMS.txt, mostra o hash,
 * pede confirmação, assina com a chave local e envia SOMENTE o .sig. `gh(args)` executa o GitHub CLI e devolve a saída.
 */
export async function signRelease({ tag, gh, keyFile, confirm, trusted = trustedKeys(), resign = false, log = console.log }) {
  const version = tag.startsWith("v") ? tag.slice(1) : "";
  if (!isVersion(version)) throw new Error(`invalid tag: ${tag} (expected vX.Y.Z)`);
  const info = JSON.parse(gh(["release", "view", tag, "--json", "assets,isDraft"]));
  const names = info.assets.map((a) => a.name);
  const installers = names.filter((n) => n.endsWith("-setup.exe"));
  if (installers.length !== 1) throw new Error(`expected exactly one *-setup.exe asset in ${tag}, found ${installers.length}`);
  const installer = installers[0];
  if (!names.includes("SHA256SUMS.txt")) throw new Error(`${tag} has no SHA256SUMS.txt asset`);
  if (names.includes(`${installer}.sig`) && !resign) throw new Error(`${installer} is already signed (use --resign to replace the signature)`);

  const dir = mkdtempSync(join(tmpdir(), "bb-release-sign-"));
  try {
    gh(["release", "download", tag, "--dir", dir, "--pattern", installer, "--pattern", "SHA256SUMS.txt", "--clobber"]);
    const bytes = readFileSync(join(dir, installer));
    const sha = sha256Hex(bytes);
    const lines = readFileSync(join(dir, "SHA256SUMS.txt"), "utf8").split(/\r?\n/);
    const listed = lines.map((l) => l.trim()).find((l) => l.endsWith("  " + installer));
    if (!listed || listed.split(/\s+/)[0].toLowerCase() !== sha) {
      throw new Error("the downloaded installer does not match SHA256SUMS.txt; refusing to sign");
    }
    log(`Release   ${tag}
Installer ${installer} (${bytes.length} bytes)
SHA-256   ${sha}  (matches SHA256SUMS.txt)`);
    if ((await confirm(`Sign this installer as version ${version}? Type "yes" to continue: `)).trim().toLowerCase() !== "yes") {
      throw new Error("cancelled; nothing was signed or uploaded");
    }
    const key = createPrivateKey(readFileSync(keyFile, "utf8"));
    const sig = signFile(key, version, bytes);
    if (!verifyFile(trusted, version, bytes, sig)) {
      throw new Error("this signing key is not among the app's trusted public keys; the app would reject the update");
    }
    const sigPath = join(dir, `${installer}.sig`);
    writeFileSync(sigPath, `${sig}
`);
    gh(["release", "upload", tag, sigPath, "--clobber"]);
    log(`Uploaded ${basename(sigPath)} to ${tag}. The release is now installable by apps that verify signatures.`);
    return { sha, installer, sig };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function loadPrivateKey(args) {
  const i = args.indexOf("--key-file");
  const pem = i >= 0 ? readFileSync(args[i + 1], "utf8") : process.env.BB_SIGNING_KEY;
  if (!pem) throw new Error("no signing key: set BB_SIGNING_KEY or pass --key-file <file>");
  return createPrivateKey(pem);
}

async function main([cmd, a, b, ...rest]) {
  if (cmd === "keygen") {
    if (!a) throw new Error("usage: keygen <private-key-file>");
    assertOutsideRepo(a);
    if (existsSync(a)) throw new Error(`${a} already exists; refusing to overwrite a key`);
    const { privateKey } = generateKeyPairSync("ed25519");
    writeFileSync(a, privateKey.export({ type: "pkcs8", format: "pem" }), { mode: 0o600, flag: "wx" });
    console.log(`Private key written to ${a} (keep it secret and back it up; it is never printed).`);
    console.log(`Public key (paste into ${VERIFY_RS}):\n${publicKeyHex(privateKey)}`);
    return;
  }
  if (cmd === "sign") {
    if (!a || !b) throw new Error("usage: sign <installer> <version> [--key-file <file>]");
    const key = loadPrivateKey(rest);
    const bytes = readFileSync(a);
    const sig = signFile(key, b, bytes);
    // Trava de segurança: uma assinatura que o app não aceitaria não pode ser publicada.
    if (!verifyFile(trustedKeys(), b, bytes, sig)) {
      throw new Error("this signing key is not among the app's trusted public keys; the app would reject the update");
    }
    writeFileSync(`${a}.sig`, `${sig}\n`);
    console.log(`Signed ${a} as version ${b}\nsha256  ${sha256Hex(bytes)}\nwrote   ${a}.sig`);
    return;
  }
  if (cmd === "verify") {
    if (!a || !b) throw new Error("usage: verify <installer> <version>");
    const sig = readFileSync(`${a}.sig`, "utf8").trim();
    if (!verifyFile(trustedKeys(), b, readFileSync(a), sig)) throw new Error("signature does NOT match");
    console.log("OK: signature is valid for this installer and version.");
    return;
  }
  if (cmd === "release") {
    if (!a) throw new Error("usage: release <tag> [--key-file <file>] [--resign]");
    const args = [b, ...rest].filter(Boolean);
    const i = args.indexOf("--key-file");
    const rl = createInterface({ input: process.stdin, output: process.stdout });
    try {
      await signRelease({
        tag: a,
        gh: (g) => execFileSync("gh", g, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 }),
        keyFile: i >= 0 ? args[i + 1] : DEFAULT_KEY_FILE,
        confirm: (q) => rl.question(q),
        resign: args.includes("--resign"),
      });
    } finally {
      rl.close();
    }
    return;
  }
  throw new Error("usage: sign-release.mjs keygen <file> | sign <installer> <version> | verify <installer> <version> | release <tag>");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(`FAIL: ${e.message}`);
    process.exit(1);
  });
}
