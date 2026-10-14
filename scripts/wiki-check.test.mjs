// Conferência da wiki bilíngue (scripts/wiki-check.mjs): só páginas sintéticas.
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { check } from "./wiki-check.mjs";

const GOOD = {
  "Home.md": "# Wiki\n\n[Português](PT-Inicio) [Start](Getting-Started)\n\nThis wiki describes version **1.2.3**.\n",
  "PT-Inicio.md": "# Wiki\n\n[English](Home) [Começar](PT-Primeiros-Passos)\n\nEsta wiki descreve a versão **1.2.3**.\n",
  "Getting-Started.md": "# Start\n\n[Português](PT-Primeiros-Passos) [Home](Home)\n",
  "PT-Primeiros-Passos.md": "# Começar\n\n[English](Getting-Started) [Início](PT-Inicio)\n",
  "_Sidebar.md": "- [Home](Home)\n- [Início](PT-Inicio)\n",
};

function wiki(overrides = {}) {
  const dir = mkdtempSync(join(tmpdir(), "bb-wiki-"));
  const files = { ...GOOD, ...overrides };
  for (const [name, text] of Object.entries(files)) if (text !== null) writeFileSync(join(dir, name), text);
  return dir;
}

function run(overrides, version = "1.2.3") {
  const dir = wiki(overrides);
  try {
    return check(dir, version);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a consistent bilingual wiki has no problems", () => {
  assert.deepEqual(run({}), []);
});

test("the version on both home pages must be the release version", () => {
  const p = run({}, "1.2.4");
  assert.ok(p.some((x) => x.startsWith("Home.md")) && p.some((x) => x.startsWith("PT-Inicio.md")), p.join("|"));
});

test("dashes are rejected in any page, in either language", () => {
  for (const ch of ["\u2014", "\u2013"]) {
    const p = run({ "Getting-Started.md": `# Start ${ch} here\n\n[Português](PT-Primeiros-Passos)\n` });
    assert.ok(p.some((x) => x.includes("travessão")), `${ch}: ${p.join("|")}`);
  }
});

test("a page that only exists in one language, or without a link to the other, is reported", () => {
  assert.ok(run({ "PT-Primeiros-Passos.md": null, "Getting-Started.md": "# Start\n\n[Home](Home)\n" }).some((x) => /idioma|quantidade/.test(x)));
  assert.ok(run({ "Getting-Started.md": "# Start\n\n[Home](Home)\n" }).some((x) => x.includes("sem link para a versão no outro idioma")));
});

test("broken internal links are reported but external links are not", () => {
  const p = run({ "Getting-Started.md": "# Start\n\n[Português](PT-Primeiros-Passos) [x](Nao-Existe) [y](https://example.com/a)\n" });
  assert.deepEqual(p, ['Getting-Started.md: link quebrado para "Nao-Existe"']);
});

test("anything about the signing key or a real user path is rejected", () => {
  for (const text of ["see signing.local.json", "the keyFile field", "the private key is", "Ed25519 signature", "C:\\Users\\someone\\x", "TRUSTED_PUBLIC_KEYS"]) {
    const p = run({ "Getting-Started.md": `# Start\n\n[Português](PT-Primeiros-Passos)\n\n${text}\n` });
    assert.ok(p.length >= 1, `must reject: ${text}`);
  }
  // O espaço reservado de caminho continua permitido.
  assert.deepEqual(run({ "Getting-Started.md": "# Start\n\n[Português](PT-Primeiros-Passos)\n\n`C:\\Users\\<you>\\Apps`\n" }), []);
});

test("the Home and the Portuguese home are required", () => {
  assert.ok(run({ "Home.md": null }).some((x) => x.includes("Home.md não existe")));
  assert.ok(run({ "PT-Inicio.md": null }).some((x) => x.includes("PT-Inicio.md não existe")));
});
