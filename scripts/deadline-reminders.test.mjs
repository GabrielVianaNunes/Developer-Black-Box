// Testes dos lembretes de prazo: quando avisar, nunca repetir, quem é mencionado e as garantias do workflow.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { alreadySent, daysUntil, marker, message, parseDue, reminderFor, stageFor, todayInSaoPaulo } from "./deadline-reminders.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const issue = (body, logins = ["dono"]) => ({ number: 7, body, assignees: logins.map((login) => ({ login })) });
const body = (due) => `texto\n\n---\n**Prazo de entrega:** ${due}  (início: 2026-10-01)\n`;

test("the due date is read from the issue body and invalid dates are ignored", () => {
  assert.equal(parseDue(body("2026-10-09")), "2026-10-09");
  assert.equal(parseDue("Prazo de entrega: 2027-01-08"), "2027-01-08");
  assert.equal(parseDue("prazo de entrega:   **2026-12-04**"), "2026-12-04");
  assert.equal(parseDue("sem data nenhuma"), null);
  assert.equal(parseDue(null), null);
  assert.equal(parseDue("Prazo de entrega: 2026-02-31"), null, "a date that does not exist");
  assert.equal(parseDue("Prazo de entrega: 2026-13-01"), null);
});

test("days are counted as calendar days, including across month and year ends", () => {
  assert.equal(daysUntil("2026-10-09", "2026-10-04"), 5);
  assert.equal(daysUntil("2026-10-09", "2026-10-09"), 0);
  assert.equal(daysUntil("2026-10-09", "2026-10-10"), -1);
  assert.equal(daysUntil("2027-01-02", "2026-12-30"), 3);
  assert.equal(daysUntil("2026-03-01", "2026-02-27"), 2);
});

test("the stage is the most urgent one that has arrived: 5 days, 1 day, due day, then one notice every 3 late days", () => {
  assert.equal(stageFor(6), null);
  assert.equal(stageFor(30), null);
  for (const d of [5, 4, 3, 2]) assert.equal(stageFor(d), "T5", `${d} days`);
  assert.equal(stageFor(1), "T1");
  assert.equal(stageFor(0), "T0");
  for (const d of [-1, -2]) assert.equal(stageFor(d), "LATE0", `${d}`);
  for (const d of [-3, -4, -5]) assert.equal(stageFor(d), "LATE1", `${d}`);
  assert.equal(stageFor(-6), "LATE2");
});

test("a reminder is never repeated: the marker of the stage and due date is looked for in the comments", () => {
  const sent = [{ body: message({ assignees: ["dono"], due: "2026-10-09", days: 3, stage: "T5" }) }];
  assert.ok(alreadySent(sent, "T5", "2026-10-09"));
  assert.ok(!alreadySent(sent, "T1", "2026-10-09"), "another stage is a new notice");
  assert.ok(!alreadySent(sent, "T5", "2026-10-16"), "a new due date is a new notice");
  assert.ok(!alreadySent([{ body: "um comentário qualquer" }, { body: null }], "T5", "2026-10-09"));
  assert.equal(reminderFor(issue(body("2026-10-09")), sent, "2026-10-06"), null, "same stage, same date: nothing to send");
});

test("reminderFor sends the right notice to the assignees and skips what has no assignee, no date or is far away", () => {
  const r = reminderFor(issue(body("2026-10-09"), ["ana", "bia"]), [], "2026-10-06");
  assert.equal(r.stage, "T5");
  assert.equal(r.days, 3);
  assert.ok(r.body.includes("@ana @bia") && r.body.includes("2026-10-09") && r.body.includes("3 dias"));
  assert.ok(r.body.includes(marker("T5", "2026-10-09")));
  assert.equal(reminderFor(issue(body("2026-10-09")), [], "2026-10-08").stage, "T1");
  assert.ok(reminderFor(issue(body("2026-10-09")), [], "2026-10-08").body.includes("amanhã"));
  assert.ok(reminderFor(issue(body("2026-10-09")), [], "2026-10-09").body.includes("vence hoje"));
  const late = reminderFor(issue(body("2026-10-09")), [], "2026-10-12");
  assert.equal(late.stage, "LATE1");
  assert.ok(late.body.includes("venceu há 3 dia(s)") && late.body.includes("⚠️") && late.body.includes("Entrega"));
  assert.equal(reminderFor(issue(body("2026-10-09")), [], "2026-10-02"), null, "7 days away");
  assert.equal(reminderFor(issue(body("2026-10-09"), []), [], "2026-10-08"), null, "no assignee: nobody to mention");
  assert.equal(reminderFor(issue("sem prazo"), [], "2026-10-08"), null);
});

test("a day that was missed still gets the current notice, never several at once", () => {
  // o workflow não rodou nos dias 4 e 5: no dia 6 (3 dias antes) sai UM aviso (T5), com os dias reais
  const r = reminderFor(issue(body("2026-10-09")), [], "2026-10-06");
  assert.equal(r.stage, "T5");
  assert.ok(r.body.includes("3 dias"));
});

test("today is computed in Sao Paulo, not UTC", () => {
  assert.equal(todayInSaoPaulo(new Date("2026-10-02T02:30:00Z")), "2026-10-01", "still the evening of the 1st in Brazil");
  assert.equal(todayInSaoPaulo(new Date("2026-10-02T11:00:00Z")), "2026-10-02");
});

test("the workflow is minimal: scheduled, only reads the repository and writes issues, no secrets, no code from pull requests", () => {
  // No servidor (Windows) o checkout vem com fim de linha CRLF: normaliza antes de comparar.
  const wf = readFileSync(root + ".github/workflows/deadline-reminders.yml", "utf8").replace(/\r\n/g, "\n");
  assert.ok(/schedule:\s*\n\s*- cron: "0 11 \* \* \*"/.test(wf), "daily schedule");
  assert.ok(/workflow_dispatch:/.test(wf), "can be run by hand");
  const perms = /^permissions:\n((?: {2}\S.*\n)+)/m.exec(wf)?.[1].trim().split("\n").map((l) => l.trim());
  assert.deepEqual(perms, ["contents: read", "issues: write"], "least privilege: exactly these two permissions");
  assert.ok(!/secrets\./.test(wf), "no secrets");
  assert.ok(!/pull_request_target|pull_request:/.test(wf), "never runs on pull requests");
  assert.ok(/GH_TOKEN: \$\{\{ github\.token \}\}/.test(wf), "default workflow token only");
  assert.ok(/inputs\.dry_run/.test(wf) && /--dry-run/.test(wf), "manual runs default to a dry run");
});

test("the script only talks to the GitHub API through gh and never sends the maintainer's e-mail anywhere", () => {
  const code = readFileSync(root + "scripts/deadline-reminders.mjs", "utf8");
  assert.ok(!/fetch\(|https?:\/\/|nodemailer|smtp|@gmail/i.test(code), "no network code of its own, no addresses");
  assert.ok(/execFileSync\("gh"/.test(code));
});
