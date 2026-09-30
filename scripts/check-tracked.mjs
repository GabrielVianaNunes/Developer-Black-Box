// Falha se algum arquivo sensível estiver RASTREADO pelo Git. O .gitignore não basta:
// ele não impede que um arquivo já rastreado (ou adicionado com `git add -f`) continue no repositório.
//
// Uso: node scripts/check-tracked.mjs        (verifica os arquivos rastreados agora)
//      node scripts/check-tracked.mjs --history   (também procura no histórico inteiro)
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const FORBIDDEN = [
  { id: "database", re: /\.(db|db-wal|db-shm|sqlite3?)$/i },
  { id: "recording", re: /\.(bbseg|bbwal|corrupt|keep)$|(^|\/)(journal\.bbwal|pruned\.log|key\.bin)$/i },
  { id: "log-or-dump", re: /\.(log|dmp|mdmp|hdmp|pcap|etl)$/i },
  { id: "key-or-certificate", re: /\.(pem|key|pfx|p12|crt|cer|jks|keystore)$|(^|\/)id_(rsa|ed25519)/i },
  { id: "env-file", re: /(^|\/)\.env(\.(?!example$).*)?$/i },
  { id: "backup-or-temp", re: /\.(bak|backup|old|orig|tmp|temp|swp)$|~$/i },
  { id: "data-directory", re: /(^|\/)(segments|evidence|exports|recordings|screenshots|logs|backups|real-data)\//i },
  { id: "local-config", re: /(^|\/)(config\.local\.|settings\.local\.)|\.local\.(json|toml)$/i },
];

export function violations(paths) {
  const out = [];
  for (const p of paths) {
    const norm = p.replaceAll("\\", "/");
    for (const r of FORBIDDEN) if (r.re.test(norm)) out.push({ path: norm, rule: r.id });
  }
  return out;
}

function git(args) {
  return execFileSync("git", args, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const tracked = git(["ls-files", "-z"]).split("\0").filter(Boolean);
  let bad = violations(tracked).map((v) => ({ ...v, where: "working tree" }));
  if (process.argv.includes("--history")) {
    const everAdded = git(["log", "--all", "--name-only", "--pretty=format:", "-z"]).split("\0").filter(Boolean);
    const seen = new Set(tracked);
    bad = bad.concat(violations([...new Set(everAdded)].filter((p) => !seen.has(p))).map((v) => ({ ...v, where: "history" })));
  }
  if (bad.length) {
    console.error("Sensitive files are tracked by Git:");
    for (const b of bad) console.error(`  [${b.rule}] ${b.path} (${b.where})`);
    process.exit(1);
  }
  console.log(`OK: no sensitive files among ${tracked.length} tracked files${process.argv.includes("--history") ? " or in history" : ""}.`);
}
