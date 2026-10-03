import { useEffect, useState } from "react";
import { LanguageSwitch } from "../components/LanguageSwitch";
import { StatusLight } from "../components/StatusLight";
import { GuideModal } from "../guide/GuideModal";
import { newsFor } from "../guide/news";
import { TOUR, type Step } from "../guide/steps";
import { UnsavedBar } from "../components/UnsavedBar";
import { UpdateBanner } from "../components/UpdateBanner";
import { usePrivacyDraft } from "./usePrivacyDraft";
import { useUpdate } from "./useUpdate";
import { ActivityView } from "../features/activity/ActivityView";
import { HealthView } from "../features/health/HealthView";
import { IncidentsView } from "../features/incidents/IncidentsView";
import { OverviewView } from "../features/overview/OverviewView";
import { PrivacyView } from "../features/privacy/PrivacyView";
import { ProcessesView } from "../features/processes/ProcessesView";
import { StorageView } from "../features/storage/StorageView";
import { useI18n, type Key } from "../i18n";
import { getAppVersion, getGuideState, getStatus, markNewsSeen, markTourSeen, onNavigate, onStatus, pauseRecording, resumeRecording } from "../services/backend";
import type { Status } from "../types/status";

const VIEWS = [
  { id: "overview", label: "nav.overview" },
  { id: "activity", label: "nav.activity" },
  { id: "processes", label: "nav.processes" },
  { id: "incidents", label: "nav.incidents" },
  { id: "health", label: "nav.health" },
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
  const [version, setVersion] = useState<string | null>(null);
  const updates = useUpdate();
  const privacy = usePrivacyDraft();
  const [guideOpen, setGuideOpen] = useState(false);
  const [news, setNews] = useState<Step[] | null>(null);
  const [tourSeen, setTourSeen] = useState(true); // só abre sozinho depois de o backend dizer que é um usuário novo

  useEffect(() => {
    let alive = true;
    const unlisten: Array<() => void> = [];
    // Usuário novo: tour. Quem já viu o tour e não desligou as novidades: resumo do que mudou desde a última vez.
    Promise.all([getGuideState(), getAppVersion()])
      .then(([g, v]) => {
        if (!alive) return;
        setTourSeen(g.tourSeen);
        setGuideOpen(!g.tourSeen);
        if (g.tourSeen && g.newsEnabled) {
          const fresh = newsFor(g.seenVersion, v);
          if (fresh.length > 0) setNews(fresh);
        }
      })
      .catch(() => {});
    getAppVersion().then((v) => alive && setVersion(v)).catch(() => {});
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

  // Fechar ou pular o tour da primeira abertura o marca como visto; reabrir pelo botão "?" não muda nada.
  function closeGuide() {
    setGuideOpen(false);
    if (!tourSeen) {
      setTourSeen(true);
      void markTourSeen().catch(() => {});
    }
  }

  // Ver ou pular as novidades guarda a versão atual: elas não voltam sozinhas até a próxima atualização.
  function closeNews() {
    setNews(null);
    void markNewsSeen().catch(() => {});
  }

  const toprow = (
    <div className="toprow">
      <div className="brandrow">
        <h1 className="brand">{t("app.title")}</h1>
        {version && <span className="version muted small">{t("app.version", { version })}</span>}
      </div>
      <div className="toprow-right">
        <button className="guide-button" aria-label={t("guide.open")} title={t("guide.open")} onClick={() => setGuideOpen(true)}>?</button>
        <LanguageSwitch />
      </div>
      {guideOpen && <GuideModal steps={TOUR} onClose={closeGuide} />}
      {!guideOpen && news && version && (
        <GuideModal
          steps={news}
          onClose={closeNews}
          heading={t("guide.news.heading", { version })}
          labels={{ skip: "guide.news.skip", done: "guide.news.done" }}
        />
      )}
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
      <UnsavedBar draft={privacy} onPrivacyTab={view === "privacy"} goToPrivacy={() => setView("privacy")} />
      <UpdateBanner state={updates.state} download={updates.download} install={updates.install} installing={updates.installing} actionError={updates.actionError} />
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
      {view === "health" && <HealthView />}
      {view === "incidents" && <IncidentsView />}
      {view === "privacy" && <PrivacyView status={status} onShowNews={() => setNews(newsFor(null, version ?? ""))} privacy={privacy} />}
      {view === "storage" && <StorageView />}
    </main>
  );
}
