// Teste de interface num navegador de verdade, com o backend SIMULADO: nenhum dado real, nenhum Tauri.
// Serve a pasta `dist` (rode `npm run build` antes) e troca `window.__TAURI_INTERNALS__` por um backend de mentira que
// guarda todas as chamadas, para os testes conferirem o que a interface pediu ao backend.
//
// Navegador: `BB_UI_BROWSER` (caminho do executável) ou, sem ele, o Chrome instalado (`channel: "chrome"`; os runners do
// GitHub Actions para Windows já o trazem). Sem navegador, o teste é PULADO, exceto com `BB_UI_REQUIRE=1` (no CI), onde
// a falta de navegador é uma falha: um teste de interface que some em silêncio não protege nada.
import { existsSync, readFileSync } from "node:fs";
import http from "node:http";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("../..", import.meta.url));
const dist = join(root, "dist");
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".png": "image/png" };

export function startServer() {
  if (!existsSync(join(dist, "index.html"))) throw new Error("dist/ não existe: rode `npm run build` antes dos testes de interface");
  const server = http.createServer((req, res) => {
    const p = join(dist, req.url === "/" ? "index.html" : decodeURIComponent(req.url.split("?")[0]));
    if (!p.startsWith(dist) || !existsSync(p)) {
      res.writeHead(404);
      return res.end();
    }
    res.writeHead(200, { "content-type": types[extname(p)] ?? "application/octet-stream" });
    res.end(readFileSync(p));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve({ server, url: `http://127.0.0.1:${server.address().port}/` })));
}

/** Abre o navegador ou devolve `null` (e o motivo) se não houver um e o CI não exigir. */
export async function launchBrowser() {
  try {
    const path = process.env.BB_UI_BROWSER;
    return { browser: await chromium.launch(path ? { executablePath: path } : { channel: "chrome" }) };
  } catch (e) {
    if (process.env.BB_UI_REQUIRE === "1") throw e;
    return { skip: `sem navegador para o teste de interface (${String(e.message).split("\n")[0]})` };
  }
}

export const defaultSettings = () => ({
  protectedApps: ["browser-example.exe"],
  excludedApps: [],
  partialExclusions: [],
  excludedTrees: [],
  stabilityWindowMs: 5000,
  autoStart: false,
  retentionMaxMb: 256,
  retentionMaxHours: 24,
  telemetryEnabled: true,
  healthLogEnabled: true,
  inventoryEnabled: true,
  powerEnabled: true,
  notifyIncidents: false,
});

export const defaultHealth = () => [
  { source: "eventLog", state: "ok" },
  { source: "inventory", state: "off" },
  { source: "power", state: "unavailable" },
  { source: "telemetry", state: "waiting" },
];

/**
 * Abre o app numa página nova com o backend simulado. `opts`: `lang`, `tourSeen`, `settings`, `health`.
 * Devolve `{ page, calls, close }`; `calls` é a lista de `{ cmd, args }` que a interface mandou ao backend.
 */
export async function openApp(browser, baseUrl, opts = {}) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 }, colorScheme: "light" });
  const state = {
    lang: opts.lang ?? "en",
    tourSeen: opts.tourSeen ?? true,
    settings: { ...defaultSettings(), ...(opts.settings ?? {}) },
    health: opts.health ?? defaultHealth(),
    incident: {
      id: 7,
      kind: "blue_screen",
      severity: "critical",
      createdUtcMs: 1_000_000_000_000,
      exeName: null,
      summary: "blue_screen|209|1000000000000",
      state: "new",
      capture: "preserved",
      postUntilUtcMs: 1_000_000_300_000,
      segments: [],
    },
  };
  await page.addInitScript((init) => {
    const calls = [];
    window.__calls = calls;
    const s = init;
    const status = () => ({
      state: "Paused",
      reason: "ManualPause",
      text: s.lang === "pt-BR" ? "Pausado manualmente" : "Paused manually",
      light: "red",
      manuallyPaused: true,
      canPause: false,
      canResume: true,
    });
    const handlers = {
      get_guide_state: () => ({ tourSeen: s.tourSeen, newsEnabled: false, seenVersion: "0.5.0" }),
      mark_tour_seen: () => {
        s.tourSeen = true;
        return null;
      },
      get_app_version: () => "0.5.0",
      get_language: () => s.lang,
      set_language: (a) => {
        s.lang = a.language;
        return s.lang;
      },
      get_status: status,
      get_settings: () => s.settings,
      set_settings: (a) => {
        s.settings = a.settings;
        return s.settings;
      },
      get_config_history: () => [],
      get_health_status: () => s.health,
      get_activity: () => [],
      list_authorizations: () => [],
      get_launch_at_login: () => false,
      get_update_state: () => ({ enabled: false, checking: false, available: null, error: null, lastCheckedUtcMs: null }),
      list_app_candidates: () => [],
      list_incidents: () => [s.incident],
      get_incident: () => ({ incident: s.incident, notes: [], segments: [], timeline: [] }),
      export_incident_protected: () => ({ path: "synthetic/incident.protected.json", events: 3, dropped: 1 }),
    };
    window.__TAURI_INTERNALS__ = {
      transformCallback: () => 1,
      convertFileSrc: (x) => x,
      invoke: async (cmd, args) => {
        calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? {})) });
        if (cmd.startsWith("plugin:event")) return 1;
        return cmd in handlers ? handlers[cmd](args ?? {}) : null;
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  }, state);
  await page.goto(baseUrl);
  const calls = async () => page.evaluate(() => window.__calls);
  return { page, calls, close: () => page.close() };
}
