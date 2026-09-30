// Testes do CHANGELOG. Rode: npm test
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { promote, releaseNotes } from "./changelog.mjs";

const SAMPLE = `# Changelog

## [Unreleased]

### Added
- New thing.

## [0.1.0] - 2026-09-30

### Added
- First release.
`;

test("promote moves Unreleased into a dated section and leaves a fresh Unreleased", () => {
  const out = promote(SAMPLE, "0.2.0", "2026-10-15");
  assert.match(out, /## \[Unreleased\]\n\n## \[0\.2\.0\] - 2026-10-15\n\n### Added\n- New thing\.\n\n## \[0\.1\.0\]/);
  assert.equal(releaseNotes(out, "0.2.0"), "### Added\n- New thing.");
  assert.equal(releaseNotes(out, "0.1.0"), "### Added\n- First release.");
});

test("promote refuses empty, duplicate, malformed and missing input", () => {
  assert.throws(() => promote(SAMPLE.replace("### Added\n- New thing.\n", ""), "0.2.0", "2026-10-15"), /empty/);
  assert.throws(() => promote(SAMPLE, "0.1.0", "2026-10-15"), /already has/);
  assert.throws(() => promote(SAMPLE, "v0.2.0", "2026-10-15"), /invalid version/);
  assert.throws(() => promote(SAMPLE, "0.2.0", "yesterday"), /invalid date/);
  assert.throws(() => promote("# Changelog\n", "0.2.0", "2026-10-15"), /no \[Unreleased\]/);
});

test("promote keeps CRLF line endings", () => {
  const out = promote(SAMPLE.replaceAll("\n", "\r\n"), "0.2.0", "2026-10-15");
  assert.ok(!/[^\r]\n/.test(out), "no bare LF introduced");
  assert.equal(releaseNotes(out, "0.2.0"), "### Added\r\n- New thing.");
});

test("releaseNotes is exact about the version and ignores other sections", () => {
  assert.equal(releaseNotes(SAMPLE, "0.1.0"), "### Added\n- First release.");
  assert.equal(releaseNotes(SAMPLE, "0.1"), null);
  assert.equal(releaseNotes(SAMPLE, "0.10.0"), null);
  assert.equal(releaseNotes(SAMPLE, "9.9.9"), null);
  assert.equal(releaseNotes("## [1.0.0] - 2026-01-01\n", "1.0.0"), null, "empty section has no notes");
});

test("the real CHANGELOG has notes for the released 0.1.0", () => {
  const log = readFileSync(fileURLToPath(new URL("../CHANGELOG.md", import.meta.url)), "utf8");
  assert.ok(releaseNotes(log, "0.1.0"));
});
