// Testes do filtro da busca de programas (lógica pura da interface). Dados sintéticos.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { filterCandidates, MAX_OPTIONS } from "../src/features/privacy/filterCandidates.ts";

const app = (exe, name, extra = {}) => ({ exe, name, running: false, installed: true, ...extra });
const LIST = [
  app("chrome.exe", "Google Chrome", { running: true }),
  app("code.exe", "Visual Studio Code"),
  app("firefox.exe", "Mozilla Firefox"),
  app("notepad.exe", "Notepad"),
  app("chromedriver.exe", "ChromeDriver"),
  app("subchrome.exe", "Sub Chrome"),
  app("svchost.exe", "svchost", { installed: false, running: true }),
];
const exes = (l) => l.map((c) => c.exe);

test("matches the friendly name or the exe, case-insensitively", () => {
  assert.deepEqual(exes(filterCandidates(LIST, "GOOGLE", [])), ["chrome.exe"]);
  assert.deepEqual(exes(filterCandidates(LIST, "notepad.exe", [])), ["notepad.exe"]);
  assert.deepEqual(exes(filterCandidates(LIST, "  code ", [])), ["code.exe"]);
});

test("names that START with the text come before names that only contain it", () => {
  assert.deepEqual(exes(filterCandidates(LIST, "chrome", [])), ["chrome.exe", "chromedriver.exe", "subchrome.exe"]);
});

test("programs already in the list are not offered again", () => {
  assert.deepEqual(exes(filterCandidates(LIST, "chrome", ["chrome.exe"])), ["chromedriver.exe", "subchrome.exe"]);
  assert.ok(!exes(filterCandidates(LIST, "", ["code.exe"])).includes("code.exe"));
});

test("an empty query shows the first options, capped", () => {
  const many = Array.from({ length: 50 }, (_, i) => app(`app${i}.exe`, `App ${i}`));
  assert.equal(filterCandidates(many, "", []).length, MAX_OPTIONS);
  assert.equal(filterCandidates(many, "app", []).length, MAX_OPTIONS);
  assert.equal(filterCandidates(many, "app", [], 3).length, 3);
});

test("no match, blank and odd input never throw or invent options", () => {
  assert.deepEqual(filterCandidates(LIST, "zzz-nothing", []), []);
  assert.deepEqual(filterCandidates([], "chrome", []), []);
  for (const q of ["%", ".*", "\\", "(", "[", "?"]) assert.deepEqual(filterCandidates(LIST, q, []), [], `query ${q}`);
});

test("the picker has no free-text path: only a selected option or the file picker can add a program", () => {
  const src = readFileSync(fileURLToPath(new URL("../src/features/privacy/AppPicker.tsx", import.meta.url)), "utf8");
  // Toda adição passa por `choose` (opção da lista) ou pelo seletor nativo (`browse`); nunca pelo texto digitado.
  const adds = [...src.matchAll(/onAdd\(([^)]*)\)/g)].map((m) => m[1]);
  assert.deepEqual(adds.sort(), ["c.exe", "exe"], "onAdd is called only with a chosen candidate or the picked file");
  assert.ok(!/onAdd\(\s*query/.test(src));
});
