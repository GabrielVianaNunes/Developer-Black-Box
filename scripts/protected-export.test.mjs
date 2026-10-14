// Exportação protegida por senha: verificações de fonte (sem abrir o app) das garantias que um teste de comportamento não vê.
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8").replace(/\r\n/g, "\n");

function rustFiles(dir) {
  return readdirSync(root + dir).flatMap((f) => {
    const p = `${dir}/${f}`;
    return statSync(root + p).isDirectory() ? rustFiles(p) : p.endsWith(".rs") ? [p] : [];
  });
}

test("the app always derives the key with the production costs, and the weak test costs live only in tests", () => {
  const cmd = read("src-tauri/src/commands.rs");
  const fn = cmd.slice(cmd.indexOf("pub fn export_incident_protected"), cmd.indexOf("pub fn capture_incident"));
  assert.ok(fn.includes("KdfParams::PRODUCTION") && !fn.includes("FAST_FOR_TESTS"), "the command uses the production costs");
  for (const f of [...rustFiles("crates"), ...rustFiles("src-tauri/src")].filter((f) => !f.endsWith("protect.rs"))) {
    assert.ok(!read(f).includes("FAST_FOR_TESTS"), `${f} must not use the test-only costs`);
  }
  const protect = read("crates/bb-query/src/protect.rs");
  const productionPart = protect.slice(0, protect.indexOf("#[cfg(test)]"));
  assert.equal([...productionPart.matchAll(/FAST_FOR_TESTS/g)].length, 1, "only its definition");
  assert.ok(/PRODUCTION: KdfParams = KdfParams \{ m_cost_kib: 65_536, t_cost: 3, p_cost: 1 \}/.test(protect));
});

test("the password is never logged, stored or put in an error, in the command, the engine or the screen", () => {
  const cmd = read("src-tauri/src/commands.rs");
  const fn = cmd.slice(cmd.indexOf("pub fn export_incident_protected"), cmd.indexOf("pub fn capture_incident"));
  assert.ok(!/println!|eprintln!|log::|tracing::|dbg!|emit\(/.test(fn), "no logging or events in the command");
  const engine = read("crates/bb-engine/src/lib.rs");
  const e = engine.slice(engine.indexOf("pub fn export_incident_protected"), engine.indexOf("pub fn bug_report_summary") > 0 ? engine.indexOf("/// Resumo em texto") : undefined);
  assert.ok(!/println!|eprintln!|dbg!|set_setting|log_config_change/.test(e), "the engine does not record it");
  assert.ok(!/format!\([^)]*password/.test(e), "the password is not formatted into any text");
  const settings = read("crates/bb-engine/src/settings.rs");
  assert.ok(!/password/i.test(settings), "never a setting");
  const view = read("src/features/incidents/IncidentsView.tsx");
  assert.ok(!/localStorage|sessionStorage|indexedDB/.test(view), "nothing in the browser storage");
  assert.equal([...view.matchAll(/type="password"/g)].length, 2, "both fields hide what is typed");
  assert.equal([...view.matchAll(/autoComplete="off"/g)].length >= 2, true);
  const click = view.slice(view.indexOf("const pw = password;"), view.indexOf("const pw = password;") + 200);
  assert.ok(click.includes('setPassword("")') && click.includes('setRepeat("")'), "fields are emptied before the request goes out");
});

test("the protected format is documented and uses only standard primitives", () => {
  const protect = read("crates/bb-query/src/protect.rs");
  assert.ok(protect.includes("Argon2id") && protect.includes("Aes256Gcm"));
  assert.ok(/aad\(/.test(protect), "the header is authenticated");
  const toml = read("crates/bb-query/Cargo.toml");
  assert.ok(toml.includes("argon2") && toml.includes("aes-gcm"));
});
