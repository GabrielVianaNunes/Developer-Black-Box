// Testes do versionamento. Rode: npm test
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { bump, check, compare, isVersion, readVersions, setVersion } from "./version.mjs";

const REPO = fileURLToPath(new URL("..", import.meta.url));

/** Cópia só dos arquivos de versão, para alterar sem tocar no repositório. */
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "bb-version-"));
  for (const f of ["CHANGELOG.md", "Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "src-tauri/tauri.conf.json"]) {
    mkdirSync(join(dir, f, ".."), { recursive: true });
    cpSync(join(REPO, f), join(dir, f));
  }
  for (const m of readFileSync(join(REPO, "Cargo.toml"), "utf8").matchAll(/"((?:crates\/[^"]+)|src-tauri)"/g)) {
    mkdirSync(join(dir, m[1]), { recursive: true });
    cpSync(join(REPO, m[1], "Cargo.toml"), join(dir, m[1], "Cargo.toml"));
  }
  return dir;
}

test("the repository versions are consistent right now", () => {
  assert.deepEqual(check(REPO), []);
});

test("accepts only x.y.z and alpha/beta/rc pre-releases", () => {
  for (const ok of ["0.1.0", "1.2.3", "10.20.30", "1.0.0-rc.1", "2.0.0-beta.12"]) assert.ok(isVersion(ok), ok);
  for (const bad of ["1", "1.2", "v1.2.3", "01.2.3", "1.2.3.4", "1.2.3-rc", "1.2.3-foo.1", "1.2.3+build", "", " 1.2.3"]) {
    assert.ok(!isVersion(bad), bad);
  }
});

test("bump and compare follow semver", () => {
  assert.equal(bump("0.1.0", "patch"), "0.1.1");
  assert.equal(bump("0.1.9", "minor"), "0.2.0");
  assert.equal(bump("1.4.7", "major"), "2.0.0");
  assert.throws(() => bump("1.0.0", "huge"));
  assert.ok(compare("0.2.0", "0.1.9") > 0);
  assert.ok(compare("0.10.0", "0.9.0") > 0, "numeric, not lexical");
  assert.ok(compare("1.0.0", "1.0.0-rc.1") > 0, "final is newer than its pre-release");
  assert.ok(compare("1.0.0-rc.2", "1.0.0-rc.1") > 0);
  assert.ok(compare("1.0.0-rc.1", "1.0.0-beta.9") > 0);
  assert.equal(compare("1.2.3", "1.2.3"), 0);
});

test("set updates every file, and check only fails on the Cargo.lock until cargo refreshes it", () => {
  const dir = fixture();
  try {
    setVersion(dir, "99.9.9", { refreshLock: false });
    const v = readVersions(dir);
    assert.deepEqual([v.cargo, v.pkg, v.lock, v.lockRoot], ["99.9.9", "99.9.9", "99.9.9", "99.9.9"]);
    const problems = check(dir);
    assert.ok(problems.length > 0 && problems.every((p) => p.startsWith("Cargo.lock has")), problems.join("\n"));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("check catches each kind of drift", () => {
  const cases = [
    ["package.json", (t) => t.replace(/"version": "[^"]+"/, '"version": "9.9.9"'), /package\.json is 9\.9\.9/],
    ["src-tauri/tauri.conf.json", (t) => t.replace('"productName"', '"version": "0.1.0",\n  "productName"'), /must not set its own version/],
    ["crates/bb-core/Cargo.toml", (t) => t.replace("version.workspace = true", 'version = "0.0.1"'), /bb-core\/Cargo\.toml|crates\/bb-core/],
    ["Cargo.toml", (t) => t.replace(/(\[workspace\.package\][^[]*?version = ")[^"]+/s, "$1banana"), /not semver/],
  ];
  for (const [file, edit, expected] of cases) {
    const dir = fixture();
    try {
      writeFileSync(join(dir, file), edit(readFileSync(join(dir, file), "utf8")));
      const problems = check(dir);
      assert.ok(problems.some((p) => expected.test(p)), `${file}: ${problems.join(" | ")}`);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});

test("a release tag must match the version", () => {
  const v = readVersions(REPO).cargo;
  assert.deepEqual(check(REPO, `v${v}`), []);
  assert.ok(check(REPO, "v99.0.0").some((p) => p.includes("does not match")));
  assert.ok(check(REPO, v).some((p) => p.includes("does not match")), "tag needs the v prefix");
});

test("set refuses an invalid version", () => {
  const dir = fixture();
  try {
    assert.throws(() => setVersion(dir, "v2", { refreshLock: false }), /invalid version/);
    assert.equal(readVersions(dir).cargo, readVersions(REPO).cargo, "nothing was written");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a release needs a dated CHANGELOG section for its version", () => {
  const dir = fixture();
  try {
    const v = readVersions(dir).cargo;
    assert.deepEqual(check(dir, `v${v}`), []);
    const log = readFileSync(join(dir, "CHANGELOG.md"), "utf8");
    writeFileSync(join(dir, "CHANGELOG.md"), log.replace(`## [${v}] - `, "## [9.9.9] - "));
    assert.ok(check(dir, `v${v}`).some((p) => p.includes("CHANGELOG.md")));
    writeFileSync(join(dir, "CHANGELOG.md"), log.replace(/ - \d{4}-\d{2}-\d{2}/, " - soon"));
    assert.ok(check(dir, `v${v}`).some((p) => p.includes("CHANGELOG.md")), "needs a real date");
    assert.deepEqual(check(dir), [], "without a tag the changelog is not required");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
