// Espaço em disco (scripts/space.mjs): o guarda só apaga `target` acima do limite e a limpeza só toca artefatos.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { clean, datedBackups, dirSize, formatBytes, guard, limitFromEnv, overLimit, DEFAULT_TARGET_LIMIT_GB } from "./space.mjs";

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "bb-space-"));
  for (const d of ["target/debug", "dist", "node_modules/x", "graphify-out/2026-10-03", "graphify-out/cache", "src"]) mkdirSync(join(dir, d), { recursive: true });
  writeFileSync(join(dir, "target/debug/big.bin"), Buffer.alloc(8192));
  writeFileSync(join(dir, "dist/index.html"), "<html></html>");
  writeFileSync(join(dir, "node_modules/x/i.js"), "x");
  writeFileSync(join(dir, "graphify-out/2026-10-03/graph.json"), "{}");
  writeFileSync(join(dir, "graphify-out/graph.json"), "{}");
  writeFileSync(join(dir, "src/main.ts"), "code");
  return dir;
}

test("sizes are formatted and limits compared in a way that a bad value can never turn the guard off", () => {
  assert.equal(formatBytes(1024 ** 3), "1.00 GB");
  assert.equal(formatBytes(5 * 1024 ** 2), "5.0 MB");
  assert.equal(limitFromEnv(undefined), DEFAULT_TARGET_LIMIT_GB);
  for (const bad of ["0", "-3", "abc", "", "NaN", "Infinity"]) assert.equal(limitFromEnv(bad), DEFAULT_TARGET_LIMIT_GB, bad);
  assert.equal(limitFromEnv("2.5"), 2.5);
  assert.equal(overLimit(9 * 1024 ** 3, 8), true);
  assert.equal(overLimit(8 * 1024 ** 3, 8), false, "exactly at the limit is fine");
});

test("only date-named folders of graphify-out count as backups", () => {
  assert.deepEqual(datedBackups(["2026-10-03", "cache", "graph.json", "2026-1-3", "2026-10-03.bak"]), ["2026-10-03"]);
});

test("the guard leaves target alone under the limit and removes it over the limit, touching nothing else", () => {
  const dir = fixture();
  try {
    const logs = [];
    assert.equal(guard((l) => logs.push(l), dir, 1), false, "8 KB is under 1 GB");
    assert.ok(existsSync(join(dir, "target/debug/big.bin")));
    assert.equal(guard((l) => logs.push(l), dir, 1e-6), true, "8 KB is over ~1 KB");
    assert.ok(!existsSync(join(dir, "target")));
    for (const keep of ["dist/index.html", "node_modules/x/i.js", "graphify-out/graph.json", "src/main.ts", "graphify-out/2026-10-03/graph.json"]) {
      assert.ok(existsSync(join(dir, keep)), `${keep} must not be touched by the guard`);
    }
    assert.equal(guard(() => {}, dir, 1e-6), false, "nothing left to remove");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("clean removes build artifacts and dated graph backups, never code, dependencies or the current graph", () => {
  const dir = fixture();
  try {
    const freed = clean(() => {}, dir);
    assert.ok(freed > 8000);
    for (const gone of ["target", "dist", "graphify-out/2026-10-03"]) assert.ok(!existsSync(join(dir, gone)), `${gone} should be gone`);
    for (const keep of ["node_modules/x/i.js", "graphify-out/graph.json", "graphify-out/cache", "src/main.ts"]) {
      assert.ok(existsSync(join(dir, keep)), `${keep} must stay`);
    }
    assert.equal(readFileSync(join(dir, "src/main.ts"), "utf8"), "code");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a missing folder weighs nothing and the real repo script never deletes without the guard condition", () => {
  assert.equal(dirSize(join(tmpdir(), "bb-space-does-not-exist")), 0);
  const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.match(pkg.scripts.prebuild, /space\.mjs guard/, "building runs the guard first");
  assert.match(pkg.scripts.clean, /space\.mjs clean/);
  assert.match(pkg.scripts.space, /space\.mjs report/);
});
