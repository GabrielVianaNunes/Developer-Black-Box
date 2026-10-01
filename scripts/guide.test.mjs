// Testes do guia de boas-vindas: conteúdo completo nos dois idiomas e as garantias de segurança do guia.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { TOUR } from "../src/guide/steps.ts";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8");
const guideFiles = readdirSync(root + "src/guide").filter((f) => /[.]tsx?$/.test(f));

test("the tour has the planned steps, in order, each with a title and at least one paragraph", () => {
  assert.deepEqual(TOUR.map((s) => s.id), ["welcome", "light", "paused", "privacy", "incidents", "rules", "extras", "done"]);
  for (const s of TOUR) assert.ok(s.body.length >= 1, s.id);
});

test("every text of the tour exists in English and in Portuguese, and is not a placeholder", () => {
  const keys = TOUR.flatMap((s) => [s.title, ...s.body]);
  assert.equal(new Set(keys).size, keys.length, "no key is reused between steps");
  for (const k of keys) {
    for (const [name, dict] of [["en", en], ["pt-BR", ptBR]]) {
      assert.ok(typeof dict[k] === "string" && dict[k].trim().length > 20 || k.endsWith(".title"), `${name}: ${k}`);
      assert.ok(dict[k], `${name}: ${k} missing`);
    }
    assert.notEqual(en[k], ptBR[k], `${k} is not translated`);
  }
});

test("the guide never talks to the backend: it explains without doing", () => {
  for (const f of guideFiles) {
    const code = read(`src/guide/${f}`);
    assert.ok(!/services\/backend|@tauri-apps|invoke\(/.test(code), `${f} must not import the backend or Tauri`);
    assert.ok(!/fetch\(|XMLHttpRequest|WebSocket|navigator\.sendBeacon/.test(code), `${f} must not use the network`);
  }
});

test("every example is rendered inside the inert, labelled frame and uses made-up data only", () => {
  const demos = read("src/guide/demos.tsx");
  assert.ok(/<div className="demo-body" inert aria-hidden="true">/.test(demos), "the frame makes its content inert");
  const used = (demos.match(/<DemoFrame /g) ?? []).length;
  assert.ok(used >= TOUR.length, `each of the ${TOUR.length} steps needs a framed example (found ${used})`);
  for (const name of demos.match(/[\w-]+[.]exe/g) ?? []) {
    assert.ok(/synthetic|example/.test(name), `example data must be made up, found ${name}`);
  }
  assert.ok(/guide[.]exampleNote/.test(demos), "the frame says that it is only an illustration");
});

test("the guide can always be skipped and closes with Escape; the focus stays inside it", () => {
  const modal = read("src/guide/GuideModal.tsx");
  assert.ok(/e[.]key === "Escape"/.test(modal) && /onClose\(false\)/.test(modal));
  assert.ok(/guide[.]skip/.test(modal), "a skip button is always rendered (not behind a condition)");
  assert.ok(/aria-modal="true"/.test(modal) && /role="dialog"/.test(modal));
  assert.ok(/e[.]key !== "Tab"/.test(modal) && /e[.]shiftKey/.test(modal), "focus trap");
  assert.ok(!/disabled=\{[^}]*skip/i.test(modal), "skipping is never disabled");
});

test("the tour is marked as seen only when it was a first-time tour; the help button never marks anything", () => {
  const app = read("src/app/App.tsx");
  const uses = [...app.matchAll(/markTourSeen\(/g)].length;
  assert.equal(uses, 1, "markTourSeen is called in exactly one place");
  const closeFn = app.slice(app.indexOf("function closeGuide()"), app.indexOf("const toprow"));
  assert.ok(/if \(!tourSeen\)/.test(closeFn) && /markTourSeen\(/.test(closeFn));
  assert.ok(/onClick=\{\(\) => setGuideOpen\(true\)\}/.test(app), "the ? button only opens the guide");
  assert.ok(/setGuideOpen\(!g[.]tourSeen\)/.test(app), "it opens by itself only when the backend says the tour was not seen");
});
