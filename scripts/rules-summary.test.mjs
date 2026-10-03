// Resumo das regras em vigor e aviso de hospedeiros (#67): dados sintéticos, só um número por regra.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { isKnownHost, summarize } from "../src/features/privacy/rulesSummary.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8");

const settings = {
  protectedApps: ["synth-bank.exe", "synth-vault.exe"],
  excludedApps: ["synth-chat.exe", "synth-tool.exe"],
  partialExclusions: [{ exe: "synth-game.exe", kinds: ["metrics"] }],
  excludedTrees: ["synth-chat.exe"],
};

test("the summary lists each rule with its kind, the children option and its own count", () => {
  const s = summarize(settings, [{ exe: "synth-chat.exe", count: 7 }, { exe: "synth-game.exe", count: 2 }]);
  assert.equal(s.protectedCount, 2);
  assert.deepEqual(
    s.rows.map((r) => [r.exe, r.kind, r.children, r.omitted]),
    [
      ["synth-chat.exe", "whole", true, 7],
      ["synth-game.exe", "partial", false, 2],
      ["synth-tool.exe", "whole", false, 0],
    ],
  );
  assert.equal(s.omittedTotal, 9);
});

test("a count for a program that has no rule is ignored", () => {
  const s = summarize(settings, [{ exe: "synth-stranger.exe", count: 5 }]);
  assert.equal(s.omittedTotal, 0);
  assert.ok(!s.rows.some((r) => r.exe === "synth-stranger.exe"));
});

test("known hosts are recognised by name, ignoring case", () => {
  assert.ok(isKnownHost("msedgewebview2.exe"));
  assert.ok(isKnownHost("ApplicationFrameHost.exe"));
  assert.ok(!isKnownHost("synth-bank.exe"));
});

test("the host warning only shows on the protected list and the counter holds only a number", () => {
  const view = read("src/features/privacy/PrivacyView.tsx");
  assert.ok(/warnHosts\n/.test(view.replace(/\r\n/g, "\n")), "protected list turns the warning on");
  assert.ok(/warnHosts && items\.some\(isKnownHost\)/.test(view));
  const guard = read("crates/bb-core/src/guard.rs");
  assert.ok(/omitted: HashMap<ExeName, u64>/.test(guard), "one number per rule, nothing else");
  const dto = read("src-tauri/src/commands.rs");
  assert.ok(/pub struct OmittedDto \{\s*exe: String,\s*count: u64,\s*\}/.test(dto.replace(/\r\n/g, "\n")));
});
