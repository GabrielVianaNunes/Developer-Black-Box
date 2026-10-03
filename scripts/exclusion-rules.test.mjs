// Testes da lógica das regras de exclusão na tela. Dados sintéticos.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { canToggle, coherent, newRule, NOTHING, recordsEverything, rulesFrom, toggle, toSettings } from "../src/features/privacy/exclusionRules.ts";

const rule = (exe, lifecycle, metrics, crashes) => ({ exe, recorded: { lifecycle, metrics, crashes }, children: false });

test("a new rule excludes everything (the safe default)", () => {
  assert.deepEqual(newRule("a.exe"), { exe: "a.exe", recorded: NOTHING, children: false });
  assert.deepEqual(toSettings([newRule("a.exe")]), { excludedApps: ["a.exe"], partialExclusions: [], excludedTrees: [] });
});

test("what is recorded maps to the complement the backend stores", () => {
  const out = toSettings([rule("a.exe", false, false, true), rule("b.exe", true, false, false), rule("c.exe", true, true, false)]);
  assert.deepEqual(out.excludedApps, []);
  assert.deepEqual(out.partialExclusions, [
    { exe: "a.exe", kinds: ["lifecycle", "metrics"] }, // só falhas continuam
    { exe: "b.exe", kinds: ["metrics", "crashes"] }, // só início e fim continuam
    { exe: "c.exe", kinds: ["crashes"] }, // início/fim e CPU/memória continuam
  ]);
});

test("round trip: what is stored comes back as the same rules", () => {
  const rules = [rule("a.exe", false, false, true), rule("b.exe", true, false, false), rule("c.exe", true, true, false), newRule("d.exe")];
  const stored = toSettings(rules);
  assert.deepEqual(rulesFrom(stored), rules);
});

test("coherence: CPU and memory are never recorded without the start and end", () => {
  assert.deepEqual(coherent({ lifecycle: false, metrics: true, crashes: true }), { lifecycle: false, metrics: false, crashes: true });
  // Mesmo se os dados salvos disserem o contrário, a tela mostra o lado mais privado.
  const shown = rulesFrom({ excludedApps: [], partialExclusions: [{ exe: "a.exe", kinds: ["lifecycle"] }], excludedTrees: [] });
  assert.deepEqual(shown[0].recorded, { lifecycle: false, metrics: false, crashes: true });
});

test("toggling follows the dependency rules", () => {
  let r = newRule("a.exe");
  assert.ok(!canToggle(r, "metrics"), "CPU and memory cannot be recorded without start and end");
  assert.equal(toggle(r, "metrics"), r);
  r = toggle(r, "lifecycle");
  assert.deepEqual(r.recorded, { lifecycle: true, metrics: false, crashes: false });
  r = toggle(r, "metrics");
  assert.deepEqual(r.recorded, { lifecycle: true, metrics: true, crashes: false });
  // Desmarcar o início e fim leva CPU e memória junto.
  r = toggle(r, "lifecycle");
  assert.deepEqual(r.recorded, { lifecycle: false, metrics: false, crashes: false });
});

test("the screen cannot reach 'record everything' (that would be no rule at all)", () => {
  const two = rule("a.exe", true, true, false);
  assert.ok(!canToggle(two, "crashes"));
  assert.equal(toggle(two, "crashes"), two);
  assert.ok(recordsEverything({ lifecycle: true, metrics: true, crashes: true }));
  // Se mesmo assim chegar uma regra assim, nada é guardado em vez de uma exclusão vazia.
  assert.deepEqual(toSettings([rule("a.exe", true, true, true)]), { excludedApps: [], partialExclusions: [], excludedTrees: [] });
});

test("a full exclusion wins over a partial rule of the same program, like in the backend", () => {
  const shown = rulesFrom({ excludedApps: ["a.exe"], partialExclusions: [{ exe: "a.exe", kinds: ["metrics"] }], excludedTrees: [] });
  assert.deepEqual(shown, [newRule("a.exe")]);
});

test("rules are sorted by name and every toggle that is allowed stays coherent", () => {
  assert.deepEqual(rulesFrom({ excludedApps: ["z.exe", "a.exe"], partialExclusions: [], excludedTrees: [] }).map((r) => r.exe), ["a.exe", "z.exe"]);
  for (const l of [false, true]) {
    for (const m of [false, true]) {
      for (const c of [false, true]) {
        const start = coherent({ lifecycle: l, metrics: m, crashes: c });
        if (recordsEverything(start)) continue;
        for (const kind of ["lifecycle", "metrics", "crashes"]) {
          const next = toggle({ exe: "a.exe", recorded: start, children: false }, kind);
          assert.deepEqual(next.recorded, coherent(next.recorded), `${JSON.stringify(start)} + ${kind}`);
          assert.ok(!recordsEverything(next.recorded), `${JSON.stringify(start)} + ${kind}`);
        }
      }
    }
  }
});

// ---- excluir também os programas que o programa inicia (#65) ----
import { isExcludedWhole, setChildren } from "../src/features/privacy/exclusionRules.ts";

test("the children option exists only for a full exclusion and is stored as a separate list", () => {
  const whole = setChildren(newRule("synth-root.exe"), true);
  assert.equal(whole.children, true);
  assert.deepEqual(toSettings([whole]), { excludedApps: ["synth-root.exe"], partialExclusions: [], excludedTrees: ["synth-root.exe"] });
  assert.deepEqual(toSettings([newRule("synth-root.exe")]).excludedTrees, [], "off by default");
  // uma regra parcial não aceita a opção
  const partial = { exe: "synth-p.exe", recorded: { lifecycle: true, metrics: false, crashes: false }, children: false };
  assert.equal(setChildren(partial, true).children, false, "refused for a partial rule");
  assert.equal(isExcludedWhole(partial.recorded), false);
  assert.deepEqual(toSettings([{ ...partial, children: true }]).excludedTrees, [], "never stored for a partial rule");
});

test("recording something from the program again drops the children option", () => {
  const whole = setChildren(newRule("synth-root.exe"), true);
  const after = toggle(whole, "crashes");
  assert.equal(after.recorded.crashes, true);
  assert.equal(after.children, false, "no longer excluded as a whole: the option goes away");
});

test("the screen shows the stored option and ignores trees of programs that are not fully excluded", () => {
  const shown = rulesFrom({
    excludedApps: ["a.exe", "b.exe"],
    partialExclusions: [{ exe: "c.exe", kinds: ["metrics"] }],
    excludedTrees: ["a.exe", "c.exe", "ghost.exe"],
  });
  const byExe = Object.fromEntries(shown.map((r) => [r.exe, r.children]));
  assert.deepEqual(byExe, { "a.exe": true, "b.exe": false, "c.exe": false }, "only a full exclusion can carry the option");
});

test("round trip: the option survives the screen and back", () => {
  const stored = { excludedApps: ["a.exe", "b.exe"], partialExclusions: [], excludedTrees: ["b.exe"] };
  assert.deepEqual(toSettings(rulesFrom(stored)), stored);
});

test("the card has the checkbox, disabled unless the program is fully excluded, and the backend contract requires the field", () => {
  const card = readFileSync(new URL("../src/features/privacy/ExclusionRulesCard.tsx", import.meta.url), "utf8");
  assert.ok(/disabled=\{!isExcludedWhole\(rule\.recorded\)\}/.test(card));
  assert.ok(/setChildren\(r, e\.target\.checked\)/.test(card));
  const rust = readFileSync(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8");
  const dto = rust.slice(rust.indexOf("pub struct SettingsDto"), rust.indexOf("impl From<&Settings> for SettingsDto"));
  assert.ok(/excluded_trees: Vec<String>,/.test(dto) && !/serde\(default\)\]\s*excluded_trees/.test(dto), "required, no serde default");
});
