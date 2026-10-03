// README: imagens existentes (e só da pasta própria, com dados inventados), link de download, pasta de instalação
// recomendada e o mesmo conteúdo essencial nos dois idiomas.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const readme = readFileSync(root + "README.md", "utf8");
const [en, pt] = readme.split("\n# Português (Brasil)");

test("every image in the README exists, lives in assets/readme-images and every file there is used", () => {
  const imgs = [...readme.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)].map((m) => m[1]);
  assert.ok(imgs.length >= 3);
  for (const i of imgs) {
    assert.ok(i.startsWith("assets/readme-images/") && i.endsWith(".png"), i);
    assert.ok(existsSync(root + i), `${i} is missing`);
  }
  const files = readdirSync(root + "assets/readme-images");
  for (const f of files) assert.ok(imgs.includes(`assets/readme-images/${f}`), `${f} is not used (stale image)`);
});

test("both languages say the screenshots are made-up data, link to the latest release and recommend the install folder", () => {
  assert.ok(pt, "the Portuguese part exists");
  for (const [name, part] of [["en", en], ["pt", pt]]) {
    assert.ok(/releases\/latest/.test(part), `${name}: download link`);
    assert.ok(part.includes("Programs\\Developer Black Box"), `${name}: recommended folder`);
    assert.ok(/Program Files|Arquivos de Programas/.test(part), `${name}: folders that are refused`);
    assert.ok(/made-up|inventados/.test(part), `${name}: screenshots are made-up data`);
    assert.ok(/RELEASING\.md/.test(part), `${name}: release procedure`);
  }
});

test("the README does not carry a personal path, a private key location or an outdated claim", () => {
  assert.ok(!/C:\\Users\\(?!<)[A-Za-z0-9_.-]+/.test(readme), "no real user path");
  assert.ok(!/signing\.local|release-signing|\.key\b/i.test(readme), "nothing about the signing key");
  assert.ok(!/has not been run on GitHub|ainda não foi executado/.test(readme));
  assert.ok(!/Developer-Black-Box_0\.1\.0/.test(readme), "no hard-coded old version in the examples");
});

test("local project memory and the graphify output are ignored by Git, and the README explains an old cached icon in both languages", () => {
  const ignore = readFileSync(root + ".gitignore", "utf8");
  for (const entry of ["PROJECT_STATUS.md", "graphify-out/", ".graphify*"]) {
    assert.ok(ignore.split(/\r?\n/).includes(entry), `${entry} must be in .gitignore`);
  }
  for (const [name, part] of [["en", en], ["pt", pt]]) {
    assert.ok(part.includes("iconcache"), `${name}: the icon cache steps`);
    assert.ok(part.includes("taskkill /f /im explorer.exe"), `${name}: the explorer restart step`);
  }
});

// ---- Saúde do sistema: a documentação acompanha o código ----

const rust = readFileSync(root + "crates/bb-collector/src/healthlog.rs", "utf8").replace(/\r\n/g, "\n");
/** As regras da lista fixa do Event Log: [{ provider, id }], lidas direto do código. */
const eventRules = [...rust.matchAll(/^\s*rule\("([^"]+)",\s*(\d+),/gm)].map((m) => ({ provider: m[1], id: m[2] }));
const short = (p) => p.replace(/^Microsoft-Windows-/, "");

test("the README documents every Event Log rule of the fixed list, in both languages", () => {
  assert.ok(eventRules.length >= 20, `positive control: the rules were read from the code (${eventRules.length})`);
  for (const [lang, text] of [["EN", en], ["PT", pt]]) {
    const rows = text.split("\n").filter((l) => l.startsWith("|"));
    for (const r of eventRules) {
      const row = rows.find((l) => l.includes(short(r.provider)) && new RegExp(`(?<![\\d])${r.id}(?![\\d])`).test(l));
      assert.ok(row, `${lang}: ${r.provider} ${r.id} is not in a table row of the README`);
    }
  }
});

test("both languages have the System health section with what is read, never recorded, how to turn off and the limits", () => {
  for (const [lang, text, heads] of [
    ["EN", en, ["## System health", "### What is read", "### What is never recorded", "### When it records, and how to turn it off", "### What it cannot see without administrator rights", "### What has and has not been checked on a real Windows", "### Storage cost"]],
    ["PT", pt, ["## Saúde do sistema", "### O que é lido", "### O que nunca é gravado", "### Quando grava e como desligar", "### O que ela não enxerga sem administrador", "### O que foi e o que ainda não foi conferido num Windows real", "### Custo de armazenamento"]],
  ]) {
    for (const h of heads) assert.ok(text.includes(h), `${lang}: missing "${h}"`);
  }
});

test("the limits without administrator rights are all named, and kernel-driver sensor libraries are ruled out", () => {
  for (const [text, words] of [
    [en, ["SMART/NVMe", "TPM", "WHEA-Logger/Operational", "per-core temperature", "kernel driver", "battery wear"]],
    [pt, ["SMART/NVMe", "TPM", "WHEA-Logger/Operational", "temperatura por núcleo", "driver de kernel", "desgaste da bateria"]],
  ]) {
    for (const w of words) assert.ok(text.includes(w), `missing "${w}"`);
  }
});

test("the README names the telemetry switch exactly as the Privacy tab does", () => {
  const i18n = (f) => readFileSync(root + f, "utf8");
  const label = (src) => /"privacy\.telemetry":\s*"([^"(]+?)\s*(?:\(|")/.exec(src)[1];
  assert.ok(en.includes(`"${label(i18n("src/i18n/en.ts"))}"`), "EN switch label");
  assert.ok(pt.includes(`"${label(i18n("src/i18n/pt-BR.ts"))}"`), "PT switch label");
});

test("the README says health data is recorded during a privacy block and that the manual pause always wins", () => {
  assert.match(en, /even while a privacy block is active/);
  assert.match(en, /manual pause always wins/);
  assert.match(pt, /mesmo com um bloqueio de privacidade ativo/);
  assert.match(pt, /pausa manual sempre vence/);
});

test("the README states both what was checked on a real Windows and what was not", () => {
  assert.match(en, /\*\*Not checked\*\*, because it did not happen on that machine/);
  assert.match(pt, /\*\*Não foi conferido\*\*, porque não aconteceu naquela máquina/);
  assert.match(en, /only with synthetic XML/);
  assert.match(pt, /só testados com XML sintético/);
  assert.ok(!/have not been verified on a real machine yet/.test(en) && !/ainda não foram verificados numa máquina real/.test(pt), "the old blanket claim is gone");
});
