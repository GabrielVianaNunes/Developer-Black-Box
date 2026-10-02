// "Detectar o app em primeiro plano" (#63): a lógica das listas, o que o resultado mostra e as garantias de privacidade.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { addExclusion, addProtected, hasExclusionRule, isProtected, isSharedHost, outcome } from "../src/features/privacy/detect.ts";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8");
const settings = (extra = {}) => ({ protectedApps: ["chrome.exe"], excludedApps: [], partialExclusions: [], ...extra });

test("the result shows the name, flags the own app and never offers the own app as a name", () => {
  assert.deepEqual(outcome({ exe: "whatsapp.root.exe", isSelf: false }), { kind: "found", exe: "whatsapp.root.exe" });
  assert.deepEqual(outcome({ exe: null, isSelf: true }), { kind: "self" });
  assert.deepEqual(outcome({ exe: "developer-blackbox.exe", isSelf: true }), { kind: "self" }, "isSelf wins even if a name came along");
  assert.deepEqual(outcome({ exe: null, isSelf: false }), { kind: "unknown" });
});

test("adding to the protected list keeps it sorted and without repeats", () => {
  assert.deepEqual(addProtected(settings(), "whatsapp.root.exe"), { protectedApps: ["chrome.exe", "whatsapp.root.exe"] });
  assert.deepEqual(addProtected(settings(), "chrome.exe"), { protectedApps: ["chrome.exe"] }, "no repeat");
  assert.deepEqual(addProtected(settings({ protectedApps: ["z.exe", "b.exe"] }), "m.exe").protectedApps, ["b.exe", "m.exe", "z.exe"]);
  assert.equal(isProtected(settings(), "chrome.exe"), true);
  assert.equal(isProtected(settings(), "other.exe"), false);
});

test("adding an exclusion excludes the whole program (the safe default) and keeps the other rules", () => {
  const s = settings({ excludedApps: ["a-synthetic.exe"], partialExclusions: [{ exe: "example-editor.exe", kinds: ["metrics"] }] });
  const next = addExclusion(s, "whatsapp.root.exe");
  assert.deepEqual(next.excludedApps, ["a-synthetic.exe", "whatsapp.root.exe"]);
  assert.deepEqual(next.partialExclusions, [{ exe: "example-editor.exe", kinds: ["metrics"] }], "other rules untouched");
  // um programa com regra parcial passa a ser excluído por inteiro
  const upgraded = addExclusion(s, "example-editor.exe");
  assert.ok(upgraded.excludedApps.includes("example-editor.exe"));
  assert.equal(upgraded.partialExclusions.length, 0);
});

test("a program that already has a rule (total or partial) is recognised, so the button can be disabled", () => {
  const s = settings({ excludedApps: ["a.exe"], partialExclusions: [{ exe: "b.exe", kinds: ["crashes"] }] });
  assert.equal(hasExclusionRule(s, "a.exe"), true);
  assert.equal(hasExclusionRule(s, "b.exe"), true);
  assert.equal(hasExclusionRule(s, "c.exe"), false);
});

test("shared system hosts are recognised (case-insensitive) so the person is warned", () => {
  for (const h of ["msedgewebview2.exe", "MsEdgeWebView2.exe", "applicationframehost.exe", "explorer.exe"]) assert.equal(isSharedHost(h), true, h);
  assert.equal(isSharedHost("whatsapp.root.exe"), false);
});

test("the detection only reads a program name: no window title, no storage, no network, and the delay is clamped", () => {
  const rust = read("src-tauri/src/apps.rs");
  const cmd = rust.slice(rust.indexOf("pub async fn detect_foreground_app"), rust.indexOf("pub async fn pick_executable"));
  assert.ok(/current_foreground_exe\(\)/.test(cmd), "uses the same function as the Guard");
  assert.ok(/clamp\(1_000, 15_000\)/.test(cmd), "the wait cannot be abused");
  assert.ok(!/GetWindowText|set_setting|store|Store|fetch|http/i.test(cmd), "no title, nothing saved, no network");
  const collector = read("crates/bb-collector/src/windows_impl.rs");
  assert.ok(!/GetWindowText/.test(collector), "the collector never reads window titles");
  assert.ok(/apps::detect_foreground_app/.test(read("src-tauri/src/lib.rs")), "the command is registered");
});

test("the card lives in the Privacy tab, edits only the draft and uses the service, not the backend directly", () => {
  const view = read("src/features/privacy/PrivacyView.tsx");
  assert.ok(/<DetectForegroundCard settings=\{draft\} onChange=\{\(next\) => setDraft\(\{ \.\.\.draft, \.\.\.next \}\)\} \/>/.test(view));
  const card = read("src/features/privacy/DetectForegroundCard.tsx");
  assert.ok(/detectForegroundApp\(SECONDS \* 1000\)/.test(card));
  assert.ok(!/setSettings|invoke\(|@tauri-apps/.test(card), "it never saves: the rule only takes effect after Apply");
  assert.ok(/privacy\.detect\.added/.test(card), "tells the person it is only a draft");
  const be = read("src/services/backend.ts");
  assert.ok(/invoke<Detection>\("detect_foreground_app", \{ delayMs \}\)/.test(be));
});

test("every text of the card exists in English and in Portuguese and is translated", () => {
  const keys = Object.keys(en).filter((k) => k.startsWith("privacy.detect.") || k === "error.apps.detect_failed");
  assert.ok(keys.length >= 13, `found ${keys.length} keys`);
  for (const k of keys) {
    assert.ok(en[k] && ptBR[k], k);
    assert.notEqual(en[k], ptBR[k], `${k} is translated`);
  }
});
