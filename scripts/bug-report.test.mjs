// O botão "copiar resumo para relatar um bug": o texto vem do backend (que o monta da exportação refiltrada), a interface
// só mostra e copia por ação da pessoa. Verificações de fonte, sem abrir o app.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8").replace(/\r\n/g, "\n");

test("the summary is requested from the backend, shown for review in a read-only box and copied only on a click", () => {
  const view = read("src/features/incidents/IncidentsView.tsx");
  assert.equal([...view.matchAll(/getBugReportSummary\(/g)].length, 1, "one single call, from the button handler");
  assert.equal([...view.matchAll(/clipboard\.writeText\(/g)].length, 1, "one single copy, from the same handler");
  const handler = view.slice(view.indexOf("incidents.summaryButton") - 600, view.indexOf("incidents.summaryButton"));
  assert.ok(handler.includes("getBugReportSummary") && handler.includes("clipboard.writeText"), "both are in the click handler");
  assert.ok(/<textarea[^>]*readOnly/.test(view), "the text stays on screen, read-only, so it can be checked before pasting");
  assert.ok(!/fetch\(|XMLHttpRequest|sendBeacon|mailto:|window\.open/.test(view), "nothing leaves the app from this screen");
});

test("the command is registered and builds the text in the backend, with no file written", () => {
  const lib = read("src-tauri/src/lib.rs");
  assert.ok(lib.includes("commands::get_bug_report_summary,"), "registered");
  const cmds = read("src-tauri/src/commands.rs");
  const fn = cmds.slice(cmds.indexOf("pub fn get_bug_report_summary"), cmds.indexOf("pub fn capture_incident"));
  assert.ok(fn.includes("bug_report_summary("), "uses the engine function (which starts from the refiltered export)");
  assert.ok(!/std::fs|File::|write\(/.test(fn), "writes nothing to disk");
  const backend = read("src/services/backend.ts");
  assert.ok(backend.includes('invoke<string>("get_bug_report_summary", { id })'));
});

test("the summary is built only from the refiltered export document", () => {
  const summary = read("crates/bb-query/src/summary.rs");
  assert.ok(/pub fn bug_report_summary\(doc: &ExportDoc/.test(summary), "its input is the export document");
  assert.ok(!/Recorder|Store|read_segment|journal_lines|list_notes/.test(summary.split("#[cfg(test)]")[0]), "never reads recorded data or notes directly");
  const engine = read("crates/bb-engine/src/lib.rs");
  const fn = engine.slice(engine.indexOf("pub fn bug_report_summary"), engine.indexOf("/// Exclui atividade gravada"));
  assert.ok(fn.includes("self.export_doc("), "the engine gets the document from the same function the export uses");
});
