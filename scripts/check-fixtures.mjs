// Higiene das fixtures de teste (tests/fixtures): o repositório é público, então nada ali pode parecer dado real.
// Reprova nomes de computador/usuário sem marca sintética, e-mails, MAC, GUID e SID não zerados, endereços IP de rede
// privada, números de série e nomes padrão de computador do Windows. Não substitui o scanner de segredos: soma a ele.
//
// Uso: node scripts/check-fixtures.mjs
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const SYNTH = /synth|example/i;

const CHECKS = [
  {
    id: "computer-name",
    test: (t) => [...t.matchAll(/<Computer>([^<]*)<\/Computer>/g)].filter((m) => !/^SYNTH/.test(m[1])).map((m) => m[1]),
  },
  {
    id: "user-folder",
    test: (t) => [...t.matchAll(/(?:[A-Za-z]:\\+Users\\+|\/home\/|\/Users\/)([^\\/\s<>'"]+)/g)].filter((m) => !SYNTH.test(m[1])).map((m) => m[1]),
  },
  { id: "email", test: (t) => t.match(/\b[\w.+-]+@[\w-]+\.[\w.-]+\b/g) ?? [] },
  { id: "mac-address", test: (t) => t.match(/\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b/g) ?? [] },
  {
    id: "guid",
    test: (t) => (t.match(/\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b/g) ?? []).filter((g) => /[1-9a-fA-F]/.test(g)),
  },
  {
    id: "sid",
    test: (t) => (t.match(/\bS-1-5-21-\d+-\d+-\d+-\d+\b/g) ?? []).filter((s) => !/^S-1-5-21-0-0-0-\d+$/.test(s)),
  },
  {
    id: "private-ip",
    test: (t) => t.match(/\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b/g) ?? [],
  },
  { id: "default-computer-name", test: (t) => t.match(/\b(?:DESKTOP|LAPTOP)-[A-Z0-9]{7}\b/g) ?? [] },
  {
    id: "serial-number",
    test: (t) =>
      t.split(/\r?\n/).filter((l) => /serial/i.test(l) && !SYNTH.test(l)).map((l) => l.trim().slice(0, 60)),
  },
];

/** Os achados de um texto de fixture: [{ rule, value }]. Vazio = limpo. */
export function findProblems(text) {
  const out = [];
  for (const c of CHECKS) for (const value of c.test(text)) out.push({ rule: c.id, value: String(value).slice(0, 60) });
  return out;
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const files = execFileSync("git", ["ls-files", "-z", "tests/fixtures"], { encoding: "utf8" }).split("\0").filter(Boolean);
  const problems = [];
  for (const f of files) {
    let text;
    try {
      text = readFileSync(f, "utf8");
    } catch {
      continue;
    }
    for (const p of findProblems(text)) problems.push({ file: f, ...p });
  }
  if (problems.length) {
    console.error(`FAIL: ${problems.length} fixture value(s) look like real data:`);
    for (const p of problems) console.error(`  ${p.file}: [${p.rule}] ${p.value}`);
    process.exit(1);
  }
  console.log(`OK: ${files.length} fixture file(s) hold only synthetic data.`);
}
