// CHANGELOG.md: texto da Release e preparação de uma nova versão.
//
// Uso: node scripts/changelog.mjs notes 0.2.0      imprime o texto da seção [0.2.0] (vira as notas da Release)
//      node scripts/changelog.mjs promote 0.2.0    transforma [Unreleased] em [0.2.0] - <hoje> e abre um novo [Unreleased]
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { isVersion } from "./version.mjs";

const esc = (v) => v.replaceAll(".", "\\.");
const UNTIL_NEXT_SECTION = "([\\s\\S]*?)(?=^## \\[|(?![\\s\\S]))";

/** Corpo de `## [version] - data` (sem o título), ou null se a seção não existir ou estiver vazia. */
export function releaseNotes(log, version) {
  const head = "^## \\[" + esc(version) + "\\] - \\d{4}-\\d{2}-\\d{2}[^\\S\\n]*\\r?\\n";
  const body = new RegExp(head + UNTIL_NEXT_SECTION, "m").exec(log)?.[1].trim();
  return body ? body : null;
}

/** Move o conteúdo de [Unreleased] para uma seção datada da versão. Falha se não houver nada a lançar. */
export function promote(log, version, date) {
  if (!isVersion(version)) throw new Error(`invalid version: ${version}`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`invalid date: ${date}`);
  if (new RegExp("^## \\[" + esc(version) + "\\]", "m").test(log)) throw new Error(`CHANGELOG.md already has ${version}`);
  const m = new RegExp("^## \\[Unreleased\\][^\\S\\n]*\\r?\\n" + UNTIL_NEXT_SECTION, "m").exec(log);
  if (!m) throw new Error("CHANGELOG.md has no [Unreleased] section");
  if (!m[1].trim()) throw new Error("[Unreleased] is empty: nothing to release");
  const eol = log.includes("\r\n") ? "\r\n" : "\n";
  const replacement = `## [Unreleased]${eol}${eol}## [${version}] - ${date}${eol}${eol}${m[1].trim()}${eol}${eol}`;
  return log.slice(0, m.index) + replacement + log.slice(m.index + m[0].length);
}

function main([cmd, version]) {
  const path = fileURLToPath(new URL("../CHANGELOG.md", import.meta.url));
  const log = readFileSync(path, "utf8");
  if (cmd === "notes") {
    const notes = releaseNotes(log, version ?? "");
    if (!notes) throw new Error(`no notes for ${version} in CHANGELOG.md`);
    return console.log(notes);
  }
  if (cmd === "promote") {
    const today = new Date().toISOString().slice(0, 10);
    writeFileSync(path, promote(log, version ?? "", today));
    return console.log(`CHANGELOG.md: [Unreleased] -> [${version}] - ${today}`);
  }
  throw new Error("usage: changelog.mjs notes <version> | promote <version>");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`FAIL: ${e.message}`);
    process.exit(1);
  }
}
