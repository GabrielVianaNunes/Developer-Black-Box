// Testes da lógica das regras de exclusão na tela. Dados sintéticos.
import assert from "node:assert/strict";
import { test } from "node:test";
import { canToggle, coherent, newRule, NOTHING, recordsEverything, rulesFrom, toggle, toSettings } from "../src/features/privacy/exclusionRules.ts";

const rule = (exe, lifecycle, metrics, crashes) => ({ exe, recorded: { lifecycle, metrics, crashes } });

test("a new rule excludes everything (the safe default)", () => {
  assert.deepEqual(newRule("a.exe"), { exe: "a.exe", recorded: NOTHING });
  assert.deepEqual(toSettings([newRule("a.exe")]), { excludedApps: ["a.exe"], partialExclusions: [] });
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
  const shown = rulesFrom({ excludedApps: [], partialExclusions: [{ exe: "a.exe", kinds: ["lifecycle"] }] });
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
  assert.deepEqual(toSettings([rule("a.exe", true, true, true)]), { excludedApps: [], partialExclusions: [] });
});

test("a full exclusion wins over a partial rule of the same program, like in the backend", () => {
  const shown = rulesFrom({ excludedApps: ["a.exe"], partialExclusions: [{ exe: "a.exe", kinds: ["metrics"] }] });
  assert.deepEqual(shown, [newRule("a.exe")]);
});

test("rules are sorted by name and every toggle that is allowed stays coherent", () => {
  assert.deepEqual(rulesFrom({ excludedApps: ["z.exe", "a.exe"], partialExclusions: [] }).map((r) => r.exe), ["a.exe", "z.exe"]);
  for (const l of [false, true]) {
    for (const m of [false, true]) {
      for (const c of [false, true]) {
        const start = coherent({ lifecycle: l, metrics: m, crashes: c });
        if (recordsEverything(start)) continue;
        for (const kind of ["lifecycle", "metrics", "crashes"]) {
          const next = toggle({ exe: "a.exe", recorded: start }, kind);
          assert.deepEqual(next.recorded, coherent(next.recorded), `${JSON.stringify(start)} + ${kind}`);
          assert.ok(!recordsEverything(next.recorded), `${JSON.stringify(start)} + ${kind}`);
        }
      }
    }
  }
});
