// Confere a wiki do GitHub (repositório separado `<repo>.wiki.git`, clonado numa pasta) antes de publicá-la.
//
// Uso: node scripts/wiki-check.mjs <pasta-da-wiki> [versão]     (sem versão, usa a versão atual do projeto)
//
// A wiki é bilíngue e pública. Este script falha se:
//  - a versão citada na Home e na página inicial em português não for a da release;
//  - alguma página de um idioma não apontar para uma página do outro idioma;
//  - algum link interno apontar para uma página que não existe;
//  - houver travessão (— ou –) ou qualquer dado sobre chaves de assinatura ou caminhos da máquina.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readVersions } from "./version.mjs";

/** Padrões que NUNCA podem aparecer na wiki: chaves de assinatura e caminhos da máquina do mantenedor. */
export const FORBIDDEN = [
  [/signing\.local/i, "caminho do arquivo de configuração da chave de assinatura"],
  [/keyFile/, "campo do arquivo de configuração da chave"],
  [/[.]key([^A-Za-z0-9_]|$)/i, "nome de arquivo de chave"],
  [/TRUSTED_PUBLIC_KEYS/, "detalhe da chave embutida no app"],
  [/private key|chave privada/i, "referência à chave privada"],
  [/Ed25519/i, "algoritmo da chave de assinatura"],
  [/\bkey\.bin\b/i, "arquivo da chave dos dados"],
  [/[A-Za-z]:\\Users\\(?!<)[A-Za-z0-9._-]+/, "caminho com nome de usuário real"],
];

const DASHES = ["‒", "–", "—", "―", "−"];

export function pages(dir) {
  return readdirSync(dir)
    .filter((f) => f.endsWith(".md"))
    .map((f) => ({ name: f.replace(/\.md$/, ""), file: f, text: readFileSync(join(dir, f), "utf8") }));
}

const isPt = (name) => name.startsWith("PT-");
const isSpecial = (name) => name.startsWith("_");

export function check(dir, version) {
  const problems = [];
  const all = pages(dir);
  const names = new Set(all.map((p) => p.name));
  for (const required of ["Home", "PT-Inicio", "_Sidebar"]) {
    if (!names.has(required)) problems.push(`${required}.md não existe`);
  }
  for (const p of all) {
    for (const d of DASHES) if (p.text.includes(d)) problems.push(`${p.file}: travessão (U+${d.codePointAt(0).toString(16).toUpperCase()})`);
    for (const [re, why] of FORBIDDEN) if (re.test(p.text)) problems.push(`${p.file}: ${why}`);
    for (const m of p.text.matchAll(/\]\(([^)\s#]+)(?:#[^)]*)?\)/g)) {
      const target = m[1];
      if (/^(https?:|mailto:)/.test(target)) continue;
      if (!names.has(target)) problems.push(`${p.file}: link quebrado para "${target}"`);
    }
    if (!isSpecial(p.name)) {
      const links = [...p.text.matchAll(/\]\(([^)\s#]+)/g)].map((m) => m[1]).filter((t) => names.has(t));
      const other = links.some((t) => isPt(t) !== isPt(p.name) && !isSpecial(t));
      if (!other) problems.push(`${p.file}: sem link para a versão no outro idioma`);
    }
  }
  const en = all.filter((p) => !isPt(p.name) && !isSpecial(p.name)).length;
  const pt = all.filter((p) => isPt(p.name)).length;
  if (en !== pt) problems.push(`quantidade de páginas difere: ${en} em inglês e ${pt} em português`);
  const home = all.find((p) => p.name === "Home")?.text ?? "";
  const inicio = all.find((p) => p.name === "PT-Inicio")?.text ?? "";
  if (version) {
    if (!home.includes(`version **${version}**`)) problems.push(`Home.md não cita "version **${version}**"`);
    if (!inicio.includes(`versão **${version}**`)) problems.push(`PT-Inicio.md não cita "versão **${version}**"`);
  }
  return problems;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2];
  if (!dir || !existsSync(dir)) {
    console.error("uso: node scripts/wiki-check.mjs <pasta-da-wiki> [versão]");
    process.exit(2);
  }
  const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
  const version = process.argv[3] ?? readVersions(root).cargo;
  const problems = check(resolve(dir), version);
  if (problems.length) {
    for (const p of problems) console.error(`FAIL: ${p}`);
    process.exit(1);
  }
  console.log(`OK: wiki ${basename(resolve(dir))} consistente com a versão ${version}.`);
}
