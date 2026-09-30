// Scanner de segredos: chaves privadas, tokens conhecidos e atribuições de senha.
// Rode antes de cada publicação. Não substitui uma ferramenta dedicada (ex. gitleaks); é uma
// rede de proteção sem dependências para este repositório.
//
// Uso: node scripts/scan-secrets.mjs             (arquivos rastreados)
//      node scripts/scan-secrets.mjs --history   (linhas ADICIONADAS em todo o histórico)
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const RULES = [
  { id: "private-key", re: /-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----/ },
  { id: "aws-access-key", re: /\bAKIA[0-9A-Z]{16}\b/ },
  { id: "github-token", re: /\bgh[pousr]_[A-Za-z0-9]{36,}\b/ },
  { id: "slack-token", re: /\bxox[baprs]-[A-Za-z0-9-]{10,}\b/ },
  { id: "google-api-key", re: /\bAIza[0-9A-Za-z_-]{35}\b/ },
  { id: "jwt", re: /\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/ },
  { id: "secret-assignment", re: /\b(?:password|passwd|secret|api[_-]?key|token)\b\s*[:=]\s*['"][^'"\s]{8,}['"]/i },
  { id: "url-with-credentials", re: /\b[a-z][a-z0-9+.-]*:\/\/[^\s:@/]+:[^\s@/]{3,}@/i },
];

// Linhas marcadas como sintéticas/exemplo não contam (os testes de privacidade usam dados sintéticos).
const ALLOW = /SYNTH|synthetic|example|placeholder|\bdummy\b|scan-secrets:allow/i;

/** Devolve os achados de um texto: [{ rule, line, excerpt }]. */
export function scanText(text) {
  const found = [];
  text.split(/\r?\n/).forEach((line, i) => {
    if (ALLOW.test(line)) return;
    for (const r of RULES) {
      if (r.re.test(line)) found.push({ rule: r.id, line: i + 1, excerpt: line.trim().slice(0, 60) + (line.length > 60 ? "…" : "") });
    }
  });
  return found;
}

const SKIP_FILES = /(^|\/)(package-lock\.json|Cargo\.lock)$|(^|\/)scripts\/scan-secrets(\.test)?\.mjs$|\.(png|jpe?g|ico|gif|woff2?)$/i;

function git(args) {
  return execFileSync("git", args, { encoding: "utf8", maxBuffer: 512 * 1024 * 1024 });
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const findings = [];
  for (const p of git(["ls-files", "-z"]).split("\0").filter(Boolean)) {
    if (SKIP_FILES.test(p)) continue;
    let text;
    try {
      text = readFileSync(p, "utf8");
    } catch {
      continue;
    }
    if (text.includes("\0")) continue; // binário
    for (const f of scanText(text)) findings.push({ where: p, ...f });
  }
  if (process.argv.includes("--history")) {
    // só as linhas adicionadas em cada commit
    let file = "";
    for (const line of git(["log", "--all", "-p", "--no-color", "--pretty=format:commit %h"]).split("\n")) {
      if (line.startsWith("+++ b/")) file = line.slice(6);
      else if (line.startsWith("+") && !line.startsWith("+++") && !SKIP_FILES.test(file)) {
        for (const f of scanText(line.slice(1))) findings.push({ where: `history:${file}`, ...f });
      }
    }
  }
  if (findings.length) {
    console.error("Possible secrets found:");
    for (const f of findings) console.error(`  [${f.rule}] ${f.where}:${f.line}  ${f.excerpt}`);
    process.exit(1);
  }
  console.log(`OK: no secrets found${process.argv.includes("--history") ? " (working tree and history)" : ""}.`);
}
