// Testes de interface em navegador real, com backend simulado e só dados inventados (ver harness.mjs).
// Cobrem: o guia de boas-vindas, a aba Saúde do sistema, a aba Privacidade (interruptores e alterações não salvas) e a
// troca de idioma. Rodam com `npm run build && npm run test:ui`.
import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { en } from "../../src/i18n/en.ts";
import { ptBR } from "../../src/i18n/pt-BR.ts";
import { launchBrowser, openApp, startServer } from "./harness.mjs";

let browser;
let server;
let url;
let skip;

before(async () => {
  const s = await startServer();
  server = s.server;
  url = s.url;
  const b = await launchBrowser();
  browser = b.browser;
  skip = b.skip;
  if (skip) console.log(`# ${skip}`);
});

after(async () => {
  await browser?.close();
  server?.close();
});

/** Um teste de interface: pulado só quando não há navegador e o CI não o exige (ver harness.mjs). */
function ui(name, fn) {
  test(name, async (ctx) => {
    if (skip) return ctx.skip(skip);
    await fn();
  });
}

const t = (k, vars = {}) => en[k].replace(/\{(\w+)\}/g, (_, n) => String(vars[n]));
const used = async (app, cmd) => (await app.calls()).filter((c) => c.cmd === cmd);

describe("guide", () => {
  ui("the first-run tour has 6 steps, ends with the closing button and marks the tour as seen only then", async () => {
    const app = await openApp(browser, url, { tourSeen: false });
    const dialog = app.page.getByRole("dialog");
    await dialog.waitFor();
    assert.ok(await dialog.getByText(t("guide.step", { n: 1, total: 6 })).isVisible());
    for (let n = 2; n <= 6; n++) {
      await dialog.getByRole("button", { name: t("guide.next") }).click();
      assert.ok(await dialog.getByText(t("guide.step", { n, total: 6 })).isVisible(), `step ${n}`);
    }
    assert.equal((await used(app, "mark_tour_seen")).length, 0, "not marked before the end");
    await dialog.getByRole("button", { name: t("guide.done") }).click();
    await dialog.waitFor({ state: "detached" });
    assert.equal((await used(app, "mark_tour_seen")).length, 1);
    await app.close();
  });

  ui("the guide can always be skipped, with the button or with Esc", async () => {
    for (const how of ["button", "escape"]) {
      const app = await openApp(browser, url, { tourSeen: false });
      const dialog = app.page.getByRole("dialog");
      await dialog.waitFor();
      if (how === "button") await dialog.getByRole("button", { name: t("guide.skip") }).click();
      else await app.page.keyboard.press("Escape");
      await dialog.waitFor({ state: "detached" });
      await app.close();
    }
  });

  ui("the keyboard focus stays inside the open guide", async () => {
    const app = await openApp(browser, url, { tourSeen: false });
    await app.page.getByRole("dialog").waitFor();
    for (let i = 0; i < 12; i++) {
      await app.page.keyboard.press("Tab");
      assert.ok(await app.page.evaluate(() => !!document.activeElement?.closest('[role="dialog"]')), `Tab ${i + 1} left the dialog`);
    }
    for (let i = 0; i < 6; i++) {
      await app.page.keyboard.press("Shift+Tab");
      assert.ok(await app.page.evaluate(() => !!document.activeElement?.closest('[role="dialog"]')), `Shift+Tab ${i + 1} left the dialog`);
    }
    await app.close();
  });

  ui("the guide buttons are the small ones, not the app's regular buttons", async () => {
    const app = await openApp(browser, url, { tourSeen: false });
    const dialog = app.page.getByRole("dialog");
    await dialog.waitFor();
    const size = (loc) => loc.evaluate((el) => ({ h: el.getBoundingClientRect().height, font: parseFloat(getComputedStyle(el).fontSize) }));
    const next = await size(dialog.getByRole("button", { name: t("guide.next") }));
    await dialog.getByRole("button", { name: t("guide.skip") }).click();
    const regular = await size(app.page.getByRole("button", { name: t("app.resume") }));
    assert.ok(next.h < regular.h, `guide button ${next.h}px should be lower than the regular ${regular.h}px`);
    assert.ok(next.font < regular.font, `guide font ${next.font}px should be smaller than ${regular.font}px`);
    await app.close();
  });

  ui("the ? button opens the guide for someone who already saw it, and closing it marks nothing", async () => {
    const app = await openApp(browser, url, { tourSeen: true });
    assert.equal(await app.page.getByRole("dialog").count(), 0, "no guide on its own");
    await app.page.getByRole("button", { name: t("guide.open") }).click();
    const dialog = app.page.getByRole("dialog");
    await dialog.waitFor();
    await dialog.getByRole("button", { name: t("guide.skip") }).click();
    await dialog.waitFor({ state: "detached" });
    assert.equal((await used(app, "mark_tour_seen")).length, 0);
    await app.close();
  });
});

describe("language", () => {
  ui("switching to Portuguese asks the backend and translates the screen", async () => {
    const app = await openApp(browser, url);
    await app.page.getByRole("button", { name: t("lang.ptBR") }).click();
    await app.page.getByRole("button", { name: ptBR["nav.health"] }).waitFor();
    const calls = await used(app, "set_language");
    assert.equal(calls.length, 1);
    assert.equal(calls[0].args.language, "pt-BR");
    await app.close();
  });
});

describe("system health tab", () => {
  ui("each source shows its own state, and a switched-off source is shown as off", async () => {
    const app = await openApp(browser, url);
    await app.page.getByRole("button", { name: t("nav.health") }).click();
    for (const [source, state] of [["eventLog", "ok"], ["inventory", "off"], ["power", "unavailable"], ["telemetry", "waiting"]]) {
      const row = app.page.locator("tr").filter({ has: app.page.getByText(t(`health.source.${source}`), { exact: true }) });
      assert.equal((await row.locator(".badge").innerText()).trim(), t(`health.state.${state}`), source);
    }
    await app.close();
  });
});

describe("privacy tab", () => {
  const SWITCHES = ["privacy.telemetry", "privacy.healthLog", "privacy.inventory", "privacy.power", "privacy.notify"];

  ui("the source switches and the notice switch exist, with the right defaults", async () => {
    const app = await openApp(browser, url);
    await app.page.getByRole("button", { name: t("nav.privacy") }).click();
    const expected = { "privacy.telemetry": true, "privacy.healthLog": true, "privacy.inventory": true, "privacy.power": true, "privacy.notify": false };
    for (const key of SWITCHES) {
      const box = app.page.getByLabel(t(key), { exact: true });
      await box.waitFor();
      assert.equal(await box.isChecked(), expected[key], key);
    }
    await app.close();
  });

  ui("changing a switch shows the unsaved bar; Apply sends every setting back, with only that one changed", async () => {
    const app = await openApp(browser, url);
    await app.page.getByRole("button", { name: t("nav.privacy") }).click();
    const notify = app.page.getByLabel(t("privacy.notify"), { exact: true });
    await notify.check();
    await app.page.getByText(t("privacy.unsaved.title")).waitFor();
    await app.page.getByRole("button", { name: t("privacy.apply") }).first().click();
    await app.page.getByText(t("privacy.unsaved.title")).waitFor({ state: "detached" });
    const sent = await used(app, "set_settings");
    assert.equal(sent.length, 1);
    const expected = { protectedApps: ["browser-example.exe"], excludedApps: [], partialExclusions: [], excludedTrees: [], stabilityWindowMs: 5000, autoStart: false, retentionMaxMb: 256, retentionMaxHours: 24, telemetryEnabled: true, healthLogEnabled: true, inventoryEnabled: true, powerEnabled: true, notifyIncidents: true };
    assert.deepEqual(sent[0].args.settings, expected);
    await app.close();
  });

  ui("Discard puts the switch back and sends nothing", async () => {
    const app = await openApp(browser, url);
    await app.page.getByRole("button", { name: t("nav.privacy") }).click();
    const power = app.page.getByLabel(t("privacy.power"), { exact: true });
    await power.uncheck();
    await app.page.getByText(t("privacy.unsaved.title")).waitFor();
    await app.page.getByRole("button", { name: t("privacy.discard") }).first().click();
    assert.equal(await power.isChecked(), true);
    assert.equal((await used(app, "set_settings")).length, 0);
    await app.close();
  });
});
