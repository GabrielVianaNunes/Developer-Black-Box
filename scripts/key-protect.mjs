// Proteção em repouso da chave privada de assinatura. Duas formas, ambas lidas por sign-release.mjs:
//
//   * senha (scrypt + AES-256-GCM): portátil, feita para a cópia de backup (pendrive). Sem a senha não há como
//     abrir; quem perde a senha perde a cópia.
//   * DPAPI (Windows, escopo do usuário atual): presa à sua conta neste PC, sem senha para digitar. Protege
//     contra cópia do arquivo (backup, nuvem, disco roubado); não protege contra um programa rodando como você.
//
// A chave só existe em texto claro na memória, durante a assinatura. Este arquivo é só a parte genérica
// (formato, criptografia, leitura); os comandos estão em sign-release.mjs.
import { createCipheriv, createDecipheriv, createPrivateKey, randomBytes, scrypt } from "node:crypto";
import { spawnSync } from "node:child_process";
import { closeSync, fsyncSync, openSync, readFileSync, statSync, unlinkSync, writeSync } from "node:fs";
import { promisify } from "node:util";

const scryptAsync = promisify(scrypt);

export const FORMAT_VERSION = 1;
const AAD = Buffer.from("DeveloperBlackBox-key-v1", "utf8");
const SCRYPT = { N: 2 ** 17, r: 8, p: 1, maxmem: 512 * 1024 * 1024 };
export const MIN_PASSPHRASE = 12;

/** Envelope JSON de uma chave protegida, ou null se o texto for uma chave comum (PEM) ou outra coisa. */
export function parseEnvelope(text) {
  const t = text.trimStart();
  if (!t.startsWith("{")) return null;
  let env;
  try {
    env = JSON.parse(t);
  } catch {
    return null;
  }
  if (env?.v !== FORMAT_VERSION || !["scrypt", "dpapi"].includes(env.kdf)) return null;
  return env;
}

const b64 = (buf) => Buffer.from(buf).toString("base64");
const unb64 = (s, what) => {
  if (typeof s !== "string" || s.length === 0) throw new Error(`protected key file is malformed (${what})`);
  return Buffer.from(s, "base64");
};

const normalize = (passphrase) => Buffer.from(String(passphrase).normalize("NFKC"), "utf8");

// ---- senha ----

export async function encryptWithPassphrase(pem, passphrase) {
  if (typeof passphrase !== "string" || [...passphrase].length < MIN_PASSPHRASE) {
    throw new Error(`the passphrase must have at least ${MIN_PASSPHRASE} characters`);
  }
  const salt = randomBytes(16);
  const iv = randomBytes(12);
  const key = await scryptAsync(normalize(passphrase), salt, 32, SCRYPT);
  const cipher = createCipheriv("aes-256-gcm", key, iv);
  cipher.setAAD(AAD);
  const ct = Buffer.concat([cipher.update(pem, "utf8"), cipher.final()]);
  return {
    v: FORMAT_VERSION,
    kdf: "scrypt",
    cipher: "aes-256-gcm",
    N: SCRYPT.N,
    r: SCRYPT.r,
    p: SCRYPT.p,
    salt: b64(salt),
    iv: b64(iv),
    tag: b64(cipher.getAuthTag()),
    ct: b64(ct),
  };
}

export async function decryptWithPassphrase(env, passphrase) {
  if (env?.kdf !== "scrypt") throw new Error("not a passphrase-protected key file");
  // Parâmetros do arquivo só valem dentro de limites sensatos (um arquivo adulterado não pode pedir gigabytes).
  if (env.N !== SCRYPT.N || env.r !== SCRYPT.r || env.p !== SCRYPT.p) throw new Error("unsupported key derivation parameters");
  const [salt, iv, tag, ct] = ["salt", "iv", "tag", "ct"].map((k) => unb64(env[k], k));
  if (salt.length !== 16 || iv.length !== 12 || tag.length !== 16) throw new Error("protected key file is malformed");
  const key = await scryptAsync(normalize(passphrase), salt, 32, SCRYPT);
  try {
    const d = createDecipheriv("aes-256-gcm", key, iv);
    d.setAAD(AAD);
    d.setAuthTag(tag);
    return Buffer.concat([d.update(ct), d.final()]).toString("utf8");
  } catch {
    // GCM não distingue senha errada de arquivo adulterado, e nem deve.
    throw new Error("wrong passphrase, or the protected key file was altered");
  }
}

// ---- DPAPI (Windows, usuário atual) ----

const DPAPI_SCRIPT = (op) => `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
$data = [Convert]::FromBase64String([Console]::In.ReadToEnd().Trim())
$entropy = [Text.Encoding]::UTF8.GetBytes('DeveloperBlackBox-key-v1')
$scope = [Security.Cryptography.DataProtectionScope]::CurrentUser
[Convert]::ToBase64String([Security.Cryptography.ProtectedData]::${op}($data, $entropy, $scope))
`;

function dpapi(op, inputBase64) {
  if (process.platform !== "win32") throw new Error("DPAPI is only available on Windows");
  const r = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", DPAPI_SCRIPT(op)], {
    input: inputBase64,
    encoding: "utf8",
    windowsHide: true,
  });
  if (r.status !== 0) throw new Error(`DPAPI ${op} failed (only the Windows account that protected the file can open it)`);
  return r.stdout.trim();
}

export function protectWithDpapi(pem) {
  return { v: FORMAT_VERSION, kdf: "dpapi", scope: "current-user", data: dpapi("Protect", b64(Buffer.from(pem, "utf8"))) };
}

export function unprotectWithDpapi(env) {
  if (env?.kdf !== "dpapi") throw new Error("not a DPAPI-protected key file");
  return Buffer.from(dpapi("Unprotect", unb64(env.data, "data").toString("base64")), "base64").toString("utf8");
}

// ---- leitura ----

/** Lê a chave privada de um arquivo: PEM comum, protegido por senha ou por DPAPI. Só devolve um KeyObject. */
export async function readKeyFile(file, { passphrase } = {}) {
  const text = readFileSync(file, "utf8");
  const env = parseEnvelope(text);
  if (!env) return createPrivateKey(text);
  if (env.kdf === "dpapi") return createPrivateKey(unprotectWithDpapi(env));
  const pass = typeof passphrase === "function" ? await passphrase() : passphrase;
  if (!pass) throw new Error("this key file is protected by a passphrase");
  return createPrivateKey(await decryptWithPassphrase(env, pass));
}

// ---- terminal ----

/** Pergunta sem mostrar o que é digitado. Sem terminal, usa a variável BB_KEY_PASSPHRASE (só para testes). */
export function promptHidden(question) {
  if (!process.stdin.isTTY) {
    const fromEnv = process.env.BB_KEY_PASSPHRASE;
    if (fromEnv) return Promise.resolve(fromEnv);
    return Promise.reject(new Error("a passphrase is needed but there is no terminal to ask for it"));
  }
  return new Promise((resolve, reject) => {
    process.stdout.write(question);
    const stdin = process.stdin;
    stdin.setRawMode(true);
    stdin.resume();
    stdin.setEncoding("utf8");
    let value = "";
    const done = (fn) => {
      stdin.setRawMode(false);
      stdin.pause();
      stdin.removeListener("data", onData);
      process.stdout.write("\n");
      fn();
    };
    const onData = (ch) => {
      for (const c of ch) {
        if (c === "\r" || c === "\n") return done(() => resolve(value));
        if (c.charCodeAt(0) === 3) return done(() => reject(new Error("cancelled"))); // Ctrl+C
        if (c.charCodeAt(0) === 127 || c.charCodeAt(0) === 8) value = [...value].slice(0, -1).join(""); // Backspace
        else value += c;
      }
    };
    stdin.on("data", onData);
  });
}

// ---- apagar ----

/** Sobrescreve com dados aleatórios e apaga. Em SSD o sistema pode manter cópias antigas: vale como boa prática, não como garantia. */
export function wipeFile(file) {
  const size = statSync(file).size;
  const fd = openSync(file, "r+");
  try {
    writeSync(fd, randomBytes(size), 0, size, 0);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  unlinkSync(file);
}
