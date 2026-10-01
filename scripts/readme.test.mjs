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
