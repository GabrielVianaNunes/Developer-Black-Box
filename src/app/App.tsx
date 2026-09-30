import { useEffect, useState } from "react";
import { LanguageSwitch } from "../components/LanguageSwitch";
import { StatusLight } from "../components/StatusLight";
import { ActivityView } from "../features/activity/ActivityView";
import { IncidentsView } from "../features/incidents/IncidentsView";
import { OverviewView } from "../features/overview/OverviewView";
import { PrivacyView } from "../features/privacy/PrivacyView";
import { ProcessesView } from "../features/processes/ProcessesView";
import { StorageView } from "../features/storage/StorageView";
import { useI18n, type Key } from "../i18n";
import { getStatus, onNavigate, onStatus, pauseRecording, resumeRecording } from "../services/backend";
import type { Status } from "../types/status";

const VIEWS = [
  { id: "overview", label: "nav.overview" },
  { id: "activity", label: "nav.activity" },
  { id: "processes", label: "nav.processes" },
  { id: "incidents", label: "nav.incidents" },
  { id: "privacy", label: "nav.privacy" },
  { id: "storage", label: "nav.storage" },
] as const satisfies ReadonlyArray<{ id: string; label: Key }>;

type ViewId = (typeof VIEWS)[number]["id"];

export function App() {
  const { t, errorText } = useI18n();
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [view, setView] = useState<ViewId>("overview");

  useEffect(() => {
    let alive = true;
    const unlisten: Array<() => void> = [];
    getStatus()
      .then((s) => alive && setStatus(s))
      .catch((e) => alive && setNotice(errorText(e)));
    onStatus((s) => setStatus(s)).then((u) => (alive ? unlisten.push(u) : u()));
    onNavigate((v) => VIEWS.some((x) => x.id === v) && setView(v as ViewId)).then((u) => (alive ? unlisten.push(u) : u()));
    return () => {
      alive = false;
      unlisten.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function act(fn: () => Promise<Status>, isResume: boolean) {
    setBusy(true);
    setNotice(null);
    try {
      const next = await fn();
      setStatus(next);
      // A retomada respeita o Privacy Guard: pode continuar suspensa. Explica o motivo.
      if (isResume && next.light !== "green") {
        setNotice(t("app.notResumed", { text: next.text }));
      }
    } catch {
      setNotice(t("app.actionFailed"));
    } finally {
      setBusy(false);
    }
  }

  const toprow = (
    <div className="toprow">
      <h1 className="brand">{t("app.title")}</h1>
      <LanguageSwitch />
    </div>
  );

  if (!status) {
    return (
      <main className="page">
        {toprow}
        <p className="muted">{notice ?? t("app.loading")}</p>
      </main>
    );
  }

  return (
    <main className="page">
      {toprow}
      <header className="statusbar card" aria-live="polite">
        <div className="state-row">
          <StatusLight light={status.light} />
          <div>
            <div className="state-text">{status.text}</div>
            <div className="muted small">
              {status.manuallyPaused
                ? t("app.hintManualPause")
                : status.light === "green"
                  ? t("app.hintRecording")
                  : t("app.hintBlocked")}
            </div>
          </div>
        </div>
        <div className="actions">
          <button className="primary" disabled={busy || !status.canPause} onClick={() => act(pauseRecording, false)}>
            {t("app.pause")}
          </button>
          <button className="primary" disabled={busy || !status.canResume} onClick={() => act(resumeRecording, true)}>
            {t("app.resume")}
          </button>
        </div>
        {notice && <p className="notice" role="status">{notice}</p>}
      </header>

      <nav className="tabs" aria-label={t("nav.label")}>
        {VIEWS.map((v) => (
          <button key={v.id} className={view === v.id ? "tab active" : "tab"} onClick={() => setView(v.id)}>
            {t(v.label)}
          </button>
        ))}
      </nav>

      {view === "overview" && <OverviewView status={status} />}
      {view === "activity" && <ActivityView />}
      {view === "processes" && <ProcessesView />}
      {view === "incidents" && <IncidentsView />}
      {view === "privacy" && <PrivacyView status={status} />}
      {view === "storage" && <StorageView />}
    </main>
  );
}
