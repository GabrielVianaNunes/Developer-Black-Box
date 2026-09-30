// Assinatura das Releases (Ed25519). O app só instala uma atualização cuja assinatura confere com uma
// chave pública embutida nele (crates/bb-update/src/verify.rs), então só quem tem a chave privada
// consegue publicar uma atualização que os apps aceitam.
//
// Uso: node scripts/sign-release.mjs keygen <arquivo-da-chave-privada>   gera o par; a privada NUNCA fica no repositório
//      node scripts/sign-release.mjs sign <instalador> <versão>          grava <instalador>.sig (chave: --key-file ou signing.local.json)
//      node scripts/sign-release.mjs verify <instalador> <versão>        confere o .sig com as chaves embutidas no app
//      node scripts/sign-release.mjs protect <chave> <saída> --dpapi|--passphrase [--remove-original]   protege a chave em repouso
//      node scripts/sign-release.mjs check-key [--key-file <arquivo>]     abre a chave e confere com a chave pública do app
//      node scripts/sign-release.mjs release <tag>                       baixa o instalador da Release, confere o SHA-256, assina AQUI e envia só o .sig
//
// A chave privada nunca sai do seu computador: o CI só constrói e publica o instalador; quem assina é você.
//
// A assinatura cobre "DeveloperBlackBox-release-v1\n<versão>\n<sha256 do instalador>\n": amarra o arquivo à
// versão, então um instalador antigo (assinado) não pode ser apresentado como uma versão mais nova.
import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { createInterface } from "node:readline/promises";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { encryptWithPassphrase, parseEnvelope, promptHidden, protectWithDpapi, readKeyFile, wipeFile } from "./key-protect.mjs";
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

/** Arquivo LOCAL (fora do Git, veja .gitignore) que diz onde está a chave privada: { "keyFile": "<caminho absoluto>" }. */
export const LOCAL_CONFIG = "signing.local.json";

/** Caminho da chave privada: --key-file ou, se não houver, o que está em signing.local.json. */
export function resolveKeyFile({ flag, root = ROOT } = {}) {
  if (flag) return flag;
  const config = join(root, LOCAL_CONFIG);
  if (!existsSync(config)) {
    throw new Error(`no signing key configured: create ${LOCAL_CONFIG} (local, not versioned) with {"keyFile": "<absolute path>"} or pass --key-file <file>`);
  }
  let keyFile;
  try {
    keyFile = JSON.parse(readFileSync(config, "utf8")).keyFile;
  } catch {
    throw new Error(`${LOCAL_CONFIG} is not valid JSON`);
  }
  if (typeof keyFile !== "string" || !isAbsolute(keyFile)) throw new Error(`${LOCAL_CONFIG}: "keyFile" must be an absolute path`);
  assertOutsideRepo(keyFile);
  if (!existsSync(keyFile)) throw new Error(`the key file named in ${LOCAL_CONFIG} does not exist`);
  return keyFile;
}

/**
 * Assina uma Release já publicada pelo CI: baixa o instalador, confere com o SHA256SUMS.txt, mostra o hash,
 * pede confirmação, assina com a chave local e envia SOMENTE o .sig. `gh(args)` executa o GitHub CLI e devolve a saída.
 */
const askPassphrase = () => promptHidden("Passphrase of the signing key: ");
const defaultLoadKey = (file) => readKeyFile(file, { passphrase: askPassphrase });

export async function signRelease({ tag, gh, keyFile, confirm, trusted = trustedKeys(), resign = false, log = console.log, loadKey = defaultLoadKey }) {
  const version = tag.startsWith("v") ? tag.slice(1) : "";
  if (!isVersion(version)) throw new Error(`invalid tag: ${tag} (expected vX.Y.Z)`);
  const key = await loadKey(keyFile); // antes de tocar na rede: senha errada ou arquivo ilegível falham aqui
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

async function loadPrivateKey(args) {
  const i = args.indexOf("--key-file");
  const flag = i >= 0 ? args[i + 1] : undefined;
  if (!flag && process.env.BB_SIGNING_KEY) return createPrivateKey(process.env.BB_SIGNING_KEY);
  return defaultLoadKey(resolveKeyFile({ flag }));
}

/**
 * Protege uma chave em repouso (DPAPI ou senha), confere que a cópia protegida abre e corresponde à mesma chave
 * pública e só então, se pedido, apaga o original. Nunca escreve dentro do repositório nem sobrescreve arquivos.
 */
export async function protectKeyFile({ input, output, mode, removeOriginal = false, passphrase, log = console.log }) {
  assertOutsideRepo(output);
  if (existsSync(output)) throw new Error(`${output} already exists; refusing to overwrite a key file`);
  const key = await readKeyFile(input, { passphrase: passphrase ?? askPassphrase });
  const pem = key.export({ type: "pkcs8", format: "pem" });
  let pass = null;
  if (mode === "passphrase") {
    pass = passphrase ? await passphrase() : await promptHidden("New passphrase (at least 12 characters): ");
    const again = passphrase ? pass : await promptHidden("Repeat the passphrase: ");
    if (pass !== again) throw new Error("the two passphrases do not match; nothing was written");
  }
  const envelope = mode === "dpapi" ? protectWithDpapi(pem) : await encryptWithPassphrase(pem, pass);
  writeFileSync(output, JSON.stringify(envelope, null, 2) + "\n", { flag: "wx" });
  // A cópia protegida tem de abrir e ser a MESMA chave antes de qualquer coisa ser apagada.
  const reopened = await readKeyFile(output, { passphrase: () => pass });
  if (publicKeyHex(reopened) !== publicKeyHex(key)) {
    wipeFile(output);
    throw new Error("the protected copy does not match the original key; it was discarded and nothing else was changed");
  }
  log(`Protected copy written (${mode}) and verified: it opens and is the same key.`);
  if (removeOriginal && resolve(input) !== resolve(output)) {
    if (parseEnvelope(readFileSync(input, "utf8"))) log("The original is already a protected file; removing it as requested.");
    wipeFile(input);
    log("The original file was overwritten and deleted.");
  }
  return { mode, publicKey: publicKeyHex(key) };
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
    const key = await loadPrivateKey(rest);
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
  if (cmd === "protect") {
    const [input, output] = [a, b];
    const mode = rest.includes("--dpapi") ? "dpapi" : rest.includes("--passphrase") ? "passphrase" : null;
    if (!input || !output || !mode) throw new Error("usage: protect <key file> <output file> --dpapi | --passphrase [--remove-original]");
    await protectKeyFile({ input, output, mode, removeOriginal: rest.includes("--remove-original") });
    return;
  }
  if (cmd === "check-key") {
    const i = [a, b, ...rest].indexOf("--key-file");
    const file = resolveKeyFile({ flag: i >= 0 ? [a, b, ...rest][i + 1] : undefined });
    const key = await defaultLoadKey(file);
    const trusted = trustedKeys().includes(publicKeyHex(key));
    console.log(trusted ? "OK: the key opens and matches the public key embedded in the app." : "WARNING: the key opens, but it is NOT among the app's trusted public keys.");
    if (!trusted) process.exit(1);
    return;
  }
  if (cmd === "release") {
    if (!a) throw new Error("usage: release <tag> [--key-file <file>] [--resign]");
    const args = [b, ...rest].filter(Boolean);
    const i = args.indexOf("--key-file");
    const keyFile = resolveKeyFile({ flag: i >= 0 ? args[i + 1] : undefined }); // falha cedo, antes de baixar qualquer coisa
    const rl = createInterface({ input: process.stdin, output: process.stdout });
    try {
      await signRelease({
        tag: a,
        gh: (g) => execFileSync("gh", g, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 }),
        keyFile,
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
