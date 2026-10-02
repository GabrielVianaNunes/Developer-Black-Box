// Lembretes de prazo das issues. O GitHub não envia e-mail por data de entrega; este script, rodado todo dia pelo workflow
// "Deadline reminders", comenta na issue MENCIONANDO o responsável, e o GitHub envia o e-mail de menção.
//
// A data vem da linha "Prazo de entrega: AAAA-MM-DD" no corpo da issue (a mesma do campo Entrega do projeto).
// Uso: node scripts/deadline-reminders.mjs [--dry-run] [--today AAAA-MM-DD] [--repo dono/nome]
// Precisa de GH_TOKEN (no workflow, o token padrão com `issues: write`). Só fala com a API do GitHub via `gh`.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

/** "Prazo de entrega: 2026-10-09" (com ou sem negrito) -> "2026-10-09"; data inexistente (2026-02-31) vira null. */
export function parseDue(body) {
  const m = /Prazo de entrega:\s*\**\s*(\d{4}-\d{2}-\d{2})/i.exec(body ?? "");
  if (!m) return null;
  const d = new Date(`${m[1]}T00:00:00Z`);
  return Number.isNaN(d.getTime()) || d.toISOString().slice(0, 10) !== m[1] ? null : m[1];
}

/** Dias inteiros de `today` até `due` (negativo = atrasado). Ambas AAAA-MM-DD, contadas em dias de calendário. */
export function daysUntil(due, today) {
  return Math.round((Date.parse(`${due}T00:00:00Z`) - Date.parse(`${today}T00:00:00Z`)) / 86_400_000);
}

/** A data de hoje em São Paulo (o dono do projeto está lá), não em UTC. */
export function todayInSaoPaulo(now = new Date()) {
  return new Intl.DateTimeFormat("en-CA", { timeZone: "America/Sao_Paulo", year: "numeric", month: "2-digit", day: "2-digit" }).format(now);
}

/**
 * Qual aviso vale para `days` dias de distância (só o mais urgente que já chegou; quem perdeu um dia recebe o atual):
 * T5 (até 5 dias), T1 (até 1 dia), T0 (hoje) e LATEn (atrasada: um aviso a cada 3 dias).
 */
export function stageFor(days) {
  if (days > 5) return null;
  if (days < 0) return `LATE${Math.floor(-days / 3)}`;
  if (days === 0) return "T0";
  if (days <= 1) return "T1";
  return "T5";
}

export const marker = (stage, due) => `<!-- deadline-reminder:${stage}:${due} -->`;

/** Este aviso já foi feito? (procura a marca nos comentários, para nunca repetir.) */
export function alreadySent(comments, stage, due) {
  return comments.some((c) => typeof c.body === "string" && c.body.includes(marker(stage, due)));
}

export function message({ assignees, due, days, stage }) {
  const who = assignees.map((a) => `@${a}`).join(" ");
  const when =
    stage === "T0" ? "**vence hoje**"
      : stage === "T1" ? "**vence amanhã**"
        : days > 0 ? `vence em **${days} dias**`
          : `**venceu há ${-days} dia(s)**`;
  const tail = days < 0
    ? "\n\nSe o prazo mudou, atualize a linha \"Prazo de entrega\" no corpo da issue e o campo *Entrega* do projeto."
    : "";
  return `${stage.startsWith("LATE") ? "⚠️" : "⏰"} **Lembrete de prazo** — ${who}, o prazo de entrega desta issue (${due}) ${when}.${tail}\n\n_Aviso automático (workflow Deadline reminders)._\n${marker(stage, due)}`;
}

/** Decide, para uma issue aberta, se há aviso a fazer. Devolve { stage, days, due, body } ou null. */
export function reminderFor(issue, comments, today) {
  const assignees = (issue.assignees ?? []).map((a) => a.login);
  if (assignees.length === 0) return null;
  const due = parseDue(issue.body);
  if (!due) return null;
  const days = daysUntil(due, today);
  const stage = stageFor(days);
  if (!stage || alreadySent(comments, stage, due)) return null;
  return { stage, days, due, body: message({ assignees, due, days, stage }) };
}

function gh(args, input) {
  return execFileSync("gh", args, { encoding: "utf8", input, maxBuffer: 64 * 1024 * 1024 });
}

function pages(endpoint) {
  const out = gh(["api", "--paginate", "--slurp", endpoint]);
  return JSON.parse(out).flat();
}

function main() {
  const args = process.argv.slice(2);
  const arg = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
  const dryRun = args.includes("--dry-run");
  const repo = arg("--repo") ?? process.env.GITHUB_REPOSITORY;
  if (!repo) throw new Error("Informe --repo dono/nome ou GITHUB_REPOSITORY");
  const today = arg("--today") ?? todayInSaoPaulo();
  if (!/^\d{4}-\d{2}-\d{2}$/.test(today)) throw new Error("--today deve ser AAAA-MM-DD");
  console.log(`Hoje (São Paulo): ${today}${dryRun ? "  [simulação: nada é comentado]" : ""}`);

  const issues = pages(`repos/${repo}/issues?state=open&per_page=100`).filter((i) => !i.pull_request);
  let sent = 0;
  for (const issue of issues) {
    const hasDue = parseDue(issue.body) !== null;
    if (!hasDue || (issue.assignees ?? []).length === 0) continue;
    const comments = pages(`repos/${repo}/issues/${issue.number}/comments?per_page=100`);
    const r = reminderFor(issue, comments, today);
    if (!r) continue;
    console.log(`#${issue.number} (${r.stage}, ${r.days} dia(s), prazo ${r.due}): ${dryRun ? "enviaria" : "comentando"}`);
    if (!dryRun) gh(["api", `repos/${repo}/issues/${issue.number}/comments`, "-F", "body=@-"], r.body);
    sent += 1;
  }
  console.log(`${sent} lembrete(s) ${dryRun ? "a enviar" : "enviado(s)"} (${issues.length} issues abertas verificadas).`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
