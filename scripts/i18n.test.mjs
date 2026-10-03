// Testes de internacionalização. Rode: npm test
// Importa os dicionários .ts diretamente (Node com --experimental-strip-types).
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(join(root, p), "utf8");

const keys = (o) => Object.keys(o).sort();
const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
const ACCENTS = /[áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ]/;

// Textos que legitimamente são iguais nos dois idiomas (nomes próprios, siglas, símbolos).
const SAME_IN_BOTH = new Set([
  "app.title", "lang.en", "lang.ptBR", "activity.colPid", "processes.colPid", "processes.colCpu",
  "incidents.colDelta", "detail.processMetrics", "detail.unknown", "privacy.guardTitle",
  "privacy.historyEntry", "auth.activeEntry_", "configKey.launch_at_login_", "guide.demo.box",
  "detail.inventoryChange", "inventory.item.SecureBoot", "inventory.value.uefi",
  "sample.temp", "sample.cpu", "sample.freq", "sample.commit", "sample.gpu",
]);

test("both dictionaries have exactly the same keys", () => {
  assert.deepEqual(keys(ptBR), keys(en));
});

test("no translation is empty", () => {
  for (const [k, v] of [...Object.entries(en), ...Object.entries(ptBR)]) {
    assert.ok(typeof v === "string" && v.trim().length > 0, `empty text for ${k}`);
  }
});

test("placeholders are identical in both languages for every key", () => {
  for (const k of Object.keys(en)) {
    assert.deepEqual(placeholders(ptBR[k]), placeholders(en[k]), `placeholders differ for ${k}`);
  }
});

test("English text has no Portuguese accents", () => {
  for (const [k, v] of Object.entries(en)) {
    if (k === "lang.ptBR") continue; // o nome do idioma aparece na própria língua: "Português"
    assert.ok(!ACCENTS.test(v), `Portuguese accents in English text: ${k}`);
  }
});

test("nothing was left untranslated (same text in both languages) except known proper nouns and symbols", () => {
  const same = Object.keys(en).filter((k) => en[k] === ptBR[k] && !SAME_IN_BOTH.has(k));
  assert.deepEqual(same, [], `identical in both languages, probably untranslated: ${same.join(", ")}`);
});

const srcFiles = () => {
  const out = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(ts|tsx)$/.test(name)) out.push(p);
    }
  };
  walk(join(root, "src"));
  return out.filter((p) => !/src[\\/]i18n[\\/](en|pt-BR)\.ts$/.test(p));
};

const stripComments = (s) => s.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

test("no hard-coded Portuguese text remains in the interface code", () => {
  const offenders = [];
  for (const p of srcFiles()) {
    const code = stripComments(readFileSync(p, "utf8"));
    code.split("\n").forEach((line, i) => {
      if (ACCENTS.test(line)) offenders.push(`${relative(root, p)}:${i + 1}: ${line.trim().slice(0, 70)}`);
    });
  }
  assert.deepEqual(offenders, [], "Portuguese text outside the dictionaries:\n" + offenders.join("\n"));
});

test("every translation key used in the code exists in the dictionaries", () => {
  const used = new Map();
  const add = (k, p) => used.set(k, p);
  for (const p of srcFiles()) {
    const code = stripComments(readFileSync(p, "utf8"));
    for (const m of code.matchAll(/\bt\(\s*"([^"]+)"/g)) add(m[1], p);
    for (const m of code.matchAll(/"((?:app|nav|lang|light|overview|activity|processes|incidents|privacy|startup|auth|storage)\.[A-Za-z0-9_]+)"/g)) add(m[1], p);
  }
  assert.ok(used.size > 50, "the scan should find many keys");
  for (const [k, p] of used) assert.ok(k in en, `unknown translation key "${k}" used in ${relative(root, p)}`);
});

// ---- ligação com o backend: códigos que o Rust envia precisam ter tradução na interface ----

const codesIn = (file, re) => [...read(file).matchAll(re)].map((m) => m[1]);

test("every error code the backend can send has a translation", () => {
  const re = /"((?:auth|settings|export|store|recorder|collect|incident|note|startup|language|verify|update|apps)\.[a-z_]+|internal)"/g;
  const codes = new Set([
    ...codesIn("crates/bb-engine/src/lib.rs", re),
    ...codesIn("crates/bb-engine/src/settings.rs", re),
    ...codesIn("src-tauri/src/commands.rs", re),
    ...codesIn("src-tauri/src/updates.rs", re),
    ...codesIn("src-tauri/src/apps.rs", re),
    ...codesIn("crates/bb-update/src/lib.rs", re),
    ...codesIn("crates/bb-update/src/verify.rs", re),
  ]);
  assert.ok(codes.size >= 20, `expected many error codes, found ${codes.size}`);
  for (const c of codes) assert.ok(`error.${c}` in en, `backend error code "${c}" has no "error.${c}" translation`);
});

test("every incident summary code has a translation", () => {
  const src = read("crates/bb-engine/src/incidents.rs") + read("crates/bb-engine/src/lib.rs");
  for (const code of ["manual", "cpu_sustained", "memory_high", "app_crash", "app_hang"]) {
    assert.ok(`summary.${code}` in en, `missing summary.${code}`);
    assert.ok(src.includes(`"${code}`), `the backend no longer produces "${code}": update the dictionaries`);
  }
});

test("every setting the backend logs to the change history has a translated label", () => {
  const keysFound = new Set([
    ...codesIn("crates/bb-engine/src/settings.rs", /\("([a-z_]+)",\s*"(?:added|removed|changed)"\)/g),
    ...codesIn("crates/bb-engine/src/settings.rs", /list_change\(\s*"([a-z_]+)"/g),
    ...codesIn("crates/bb-engine/src/lib.rs", /log_config_change\([^"]*"([a-z_]+)"/g),
    ...codesIn("src-tauri/src/updates.rs", /log_config_change\([^"]*"([a-z_]+)"/g),
    ...codesIn("src-tauri/src/commands.rs", /log_config_change\([^"]*"([a-z_]+)"/g),
  ]);
  assert.ok(keysFound.size >= 7, `expected the known settings, found ${[...keysFound]}`);
  for (const k of keysFound) assert.ok(`configKey.${k}` in en, `no label for configuration "${k}"`);
  for (const c of ["added", "removed", "changed"]) assert.ok(`configChange.${c}` in en);
});

test("the event kinds and incident kinds the backend produces have labels", () => {
  const kinds = codesIn("crates/bb-core/src/event.rs", /^\s{4}([A-Z][A-Za-z]+)\s*\{/gm);
  for (const k of kinds) assert.ok(`kind.${k}` in en, `no label for event kind ${k}`);
  for (const k of ["manual", "cpu_sustained", "memory_high", "unexpected_exit", "app_hang"]) {
    assert.ok(`incidentKind.${k}` in en && read("crates/bb-store/src/lib.rs").includes(`"${k}"`), `incident kind ${k}`);
  }
});
