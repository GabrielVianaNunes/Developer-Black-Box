// Alterações de privacidade não aplicadas: nunca se perdem em silêncio e sempre há um aviso visível (issue #81).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { isDirty } from "../src/features/privacy/draft.ts";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8");

const settings = (extra = {}) => ({
  protectedApps: ["chrome.exe"],
  excludedApps: [],
  partialExclusions: [],
  stabilityWindowMs: 3000,
  autoStart: false,
  ...extra,
});

test("a draft is pending only when it differs from what was applied", () => {
  const saved = settings();
  assert.equal(isDirty(null, null), false, "nothing loaded yet");
  assert.equal(isDirty(saved, null), false);
  assert.equal(isDirty(null, saved), false);
  assert.equal(isDirty(saved, settings()), false, "same content, different object");
  assert.equal(isDirty(saved, settings({ excludedApps: ["synthetic-app.exe"] })), true, "an exclusion added");
  assert.equal(isDirty(saved, settings({ protectedApps: [] })), true, "protection removed");
  assert.equal(isDirty(saved, settings({ autoStart: true })), true, "a setting changed");
  assert.equal(isDirty(saved, settings({ partialExclusions: [{ exe: "example-editor.exe", kinds: ["metrics"] }] })), true);
});

test("the draft lives at the top of the app, so switching tabs cannot discard it", () => {
  const app = read("src/app/App.tsx");
  assert.ok(/const privacy = usePrivacyDraft\(\);/.test(app), "the draft hook is used by the App");
  assert.ok(/privacy=\{privacy\}/.test(app), "the Privacy view receives it");
  const view = read("src/features/privacy/PrivacyView.tsx");
  assert.ok(!/useState<Settings/.test(view), "the Privacy view must not own the draft (it is unmounted on tab changes)");
  assert.ok(!/getSettings|setSettings/.test(view), "loading and saving happen in the hook");
});

test("the unsaved-changes bar is rendered for every tab, with Apply and Discard", () => {
  const app = read("src/app/App.tsx");
  const bar = app.indexOf("<UnsavedBar");
  const tabs = app.indexOf('<nav className="tabs"');
  const views = app.indexOf('{view === "overview"');
  assert.ok(bar > 0 && bar < views, "the bar is outside the per-tab view switch");
  assert.ok(/^\s*<UnsavedBar draft=\{privacy\} onPrivacyTab=\{view === "privacy"\}/m.test(app), "rendered unconditionally (the bar hides itself)");
  assert.ok(bar < tabs, "and above the tabs, in the same place whatever the tab");
  const comp = read("src/components/UnsavedBar.tsx");
  assert.ok(/draft\.apply\(\)/.test(comp) && /draft\.discard/.test(comp));
  assert.ok(/if \(!draft\.dirty\) return null;/.test(comp), "only while there is something unsaved");
  assert.ok(/role="alert"/.test(comp));
});

test("applying works from the bar and from the card, and both use the same functions", () => {
  const view = read("src/features/privacy/PrivacyView.tsx");
  assert.ok(/onClick=\{\(\) => void apply\(\)\}/.test(view) && /onClick=\{discard\}/.test(view));
  const hook = read("src/app/usePrivacyDraft.ts");
  assert.ok(/setSettings\(draft\)/.test(hook), "the draft is what is saved");
  assert.ok(/setSaved\(s\);\s*setDraft\(s\);/.test(hook), "after saving, the draft equals what the backend accepted");
});

test("the bar texts exist in English and in Portuguese", () => {
  for (const k of ["privacy.unsaved.title", "privacy.unsaved.text", "privacy.unsaved.review"]) {
    assert.ok(en[k] && ptBR[k], k);
    assert.notEqual(en[k], ptBR[k], `${k} is translated`);
  }
});
