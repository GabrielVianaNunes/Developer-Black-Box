// Testes do resumo de novidades depois de uma atualização: quem vê o quê, o conteúdo nos dois idiomas e as garantias.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { RELEASES, compareVersions, newsFor, parseVersion } from "../src/guide/news.ts";
import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8");

const rel = (version, id) => ({ version, steps: [{ id, title: `t.${version}`, body: [`b.${version}`] }] });
const fake = [rel("0.3.0", "picker"), rel("0.4.0", "rules"), rel("1.0.0", "done")];
const ids = (steps) => steps.map((s) => s.id);

test("versions are compared by their numbers, not as text, and pre-release tags are ignored", () => {
  assert.deepEqual(parseVersion("0.3.0-rc.1"), [0, 3, 0]);
  assert.deepEqual(parseVersion("10.20.30"), [10, 20, 30]);
  assert.equal(parseVersion("banana"), null);
  assert.equal(parseVersion("1.2"), null);
  assert.equal(compareVersions([0, 10, 0], [0, 9, 0]), 1, "0.10.0 is newer than 0.9.0 (text order would say otherwise)");
  assert.equal(compareVersions([1, 0, 0], [1, 0, 0]), 0);
  assert.equal(compareVersions([0, 2, 9], [0, 3, 0]), -1);
});

test("an update shows only the news of versions after the last one seen, up to the running version", () => {
  assert.deepEqual(ids(newsFor("0.2.0", "0.3.0", fake)), ["picker"]);
  assert.deepEqual(ids(newsFor("0.2.0", "0.4.0", fake)), ["picker", "rules"], "skipping a version shows both, oldest first");
  assert.deepEqual(ids(newsFor("0.3.0", "0.4.0", fake)), ["rules"], "what was already seen does not come back");
  assert.deepEqual(ids(newsFor("0.4.0", "0.4.0", fake)), [], "nothing new: nothing shown");
  assert.deepEqual(ids(newsFor("0.4.0", "0.3.0", fake)), [], "a downgrade shows nothing");
  assert.deepEqual(ids(newsFor("0.2.0", "0.3.5", fake)), ["picker"], "a patch with no summary of its own adds nothing");
  assert.deepEqual(ids(newsFor("0.4.0", "0.4.1", fake)), [], "a version without a summary is silent");
});

test("someone who never saw a guide gets the summaries up to the running version, and never newer ones", () => {
  assert.deepEqual(ids(newsFor(null, "0.4.0", fake)), ["picker", "rules"]);
  assert.deepEqual(ids(newsFor(null, "0.2.0", fake)), [], "the running version is older than every summary");
  assert.deepEqual(ids(newsFor(null, "0.3.0-rc.1", fake)), ["picker"]);
});

test("a version that cannot be read shows nothing instead of everything", () => {
  assert.deepEqual(newsFor("0.2.0", "", fake), []);
  assert.deepEqual(newsFor("0.2.0", "garbage", fake), []);
  assert.deepEqual(ids(newsFor("garbage", "0.3.0", fake)), ["picker"], "an unreadable last-seen value counts as never seen");
});

test("the real releases are valid, in ascending order, one entry per version", () => {
  const versions = RELEASES.map((r) => parseVersion(r.version));
  assert.ok(versions.every(Boolean));
  for (let i = 1; i < versions.length; i++) assert.equal(compareVersions(versions[i - 1], versions[i]), -1);
  for (const r of RELEASES) {
    assert.ok(r.steps.length >= 1);
    assert.equal(new Set(r.steps.map((s) => s.id)).size, r.steps.length, `${r.version}: ids unique inside a summary`);
  }
});

test("every news text exists in English and Portuguese and is translated", () => {
  const keys = RELEASES.flatMap((r) => r.steps.flatMap((s) => [s.title, ...s.body]));
  assert.equal(new Set(keys).size, keys.length, "no key reused");
  for (const k of keys) {
    assert.ok(en[k] && en[k].trim().length > 15, `en: ${k}`);
    assert.ok(ptBR[k] && ptBR[k].trim().length > 15, `pt-BR: ${k}`);
    assert.notEqual(en[k], ptBR[k], `${k} is not translated`);
  }
  for (const k of ["guide.news.heading", "guide.news.skip", "guide.news.done", "news.title", "news.checkbox", "news.help", "news.show"]) {
    assert.ok(en[k] && ptBR[k], k);
  }
});

test("every news step has an inert example to show", () => {
  const stepsTs = read("src/guide/steps.ts");
  const demos = read("src/guide/demos.tsx");
  for (const id of new Set(RELEASES.flatMap((r) => r.steps.map((s) => s.id)))) {
    assert.ok(stepsTs.includes(`"${id}"`), `${id} is a known example id`);
    assert.ok(demos.slice(demos.indexOf("export const DEMOS")).includes(` ${id}: `), `${id} has a demo`);
  }
});

test("the summary opens only for someone who already saw the tour and did not turn it off, and never over the tour", () => {
  const app = read("src/app/App.tsx");
  assert.ok(/if \(g[.]tourSeen && g[.]newsEnabled\)/.test(app), "tour seen and news enabled");
  assert.ok(/newsFor\(g[.]seenVersion, v\)/.test(app), "based on the last version seen");
  assert.ok(/\{!guideOpen && news && version && \(/.test(app), "never shown on top of the tour");
});

test("seeing or skipping the summary records the version, in one place; reopening it from Privacy does not need a flag", () => {
  const app = read("src/app/App.tsx");
  assert.equal([...app.matchAll(/markNewsSeen\(/g)].length, 1);
  const close = app.slice(app.indexOf("function closeNews()"), app.indexOf("const toprow"));
  assert.ok(/setNews\(null\)/.test(close) && /markNewsSeen\(/.test(close));
  assert.ok(/onShowNews=\{\(\) => setNews\(newsFor\(null,/.test(app), "Privacy can show it again");
  assert.ok(/labels=\{\{ skip: "guide[.]news[.]skip"/.test(app));
});

test("the opt-out lives in the Privacy tab and the news code never touches the network", () => {
  const card = read("src/features/privacy/NewsCard.tsx");
  assert.ok(/setNewsEnabled\(next\)/.test(card) && /type="checkbox"/.test(card));
  assert.ok(/<NewsCard onShow=\{onShowNews\} \/>/.test(read("src/features/privacy/PrivacyView.tsx")));
  const news = read("src/guide/news.ts");
  assert.ok(!/fetch\(|XMLHttpRequest|WebSocket|services\/backend|@tauri-apps/.test(news));
  const be = read("src/services/backend.ts");
  assert.ok(/"set_news_enabled"/.test(be) && /"mark_news_seen"/.test(be));
});
