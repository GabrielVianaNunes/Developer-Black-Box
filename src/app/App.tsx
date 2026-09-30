import { useEffect, useState } from "react";
import { StatusLight } from "../components/StatusLight";
import { ActivityView } from "../features/activity/ActivityView";
import { IncidentsView } from "../features/incidents/IncidentsView";
import { OverviewView } from "../features/overview/OverviewView";
import { PrivacyView } from "../features/privacy/PrivacyView";
import { ProcessesView } from "../features/processes/ProcessesView";
import { StorageView } from "../features/storage/StorageView";
import { getStatus, onNavigate, onStatus, pauseRecording, resumeRecording } from "../services/backend";
import type { Status } from "../types/status";

const VIEWS = [
  { id: "overview", label: "Visão geral" },
  { id: "activity", label: "Atividade" },
  { id: "processes", label: "Processos" },
  { id: "incidents", label: "Incidentes" },
  { id: "privacy", label: "Privacidade" },
  { id: "storage", label: "Armazenamento" },
] as const;

type ViewId = (typeof VIEWS)[number]["id"];

export function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [view, setView] = useState<ViewId>("overview");

  useEffect(() => {
    let alive = true;
    const unlisten: Array<() => void> = [];
    getStatus().then((s) => alive && setStatus(s)).catch(() => alive && setNotice("Não foi possível ler o estado."));
    onStatus((s) => setStatus(s)).then((u) => (alive ? unlisten.push(u) : u()));
    onNavigate((v) => VIEWS.some((x) => x.id === v) && setView(v as ViewId)).then((u) => (alive ? unlisten.push(u) : u()));
    return () => {
      alive = false;
      unlisten.forEach((u) => u());
    };
  }, []);

  async function act(fn: () => Promise<Status>, isResume: boolean) {
    setBusy(true);
    setNotice(null);
    try {
      const next = await fn();
      setStatus(next);
      // A retomada respeita o Privacy Guard: pode continuar suspensa. Explica o motivo.
      if (isResume && next.light !== "green") {
        setNotice(`A gravação não foi retomada. ${next.text}.`);
      }
    } catch {
      setNotice("A ação não pôde ser concluída.");
    } finally {
      setBusy(false);
    }
  }

  if (!status) return <main className="page">Carregando…</main>;

  return (
    <main className="page">
      <header className="statusbar card" aria-live="polite">
        <div className="state-row">
          <StatusLight light={status.light} />
          <div>
            <div className="state-text">{status.text}</div>
            <div className="muted small">
              {status.manuallyPaused
                ? "A pausa manual tem prioridade e continua até você retomar."
                : status.light === "green"
                  ? "Coletando eventos técnicos autorizados, com o Privacy Guard ativo."
                  : "O Privacy Guard mantém a gravação suspensa até o contexto ser seguro."}
            </div>
          </div>
        </div>
        <div className="actions">
          <button className="primary" disabled={busy || !status.canPause} onClick={() => act(pauseRecording, false)}>
            Pausar gravação
          </button>
          <button className="primary" disabled={busy || !status.canResume} onClick={() => act(resumeRecording, true)}>
            Retomar gravação
          </button>
        </div>
        {notice && <p className="notice" role="status">{notice}</p>}
      </header>

      <nav className="tabs" aria-label="Seções">
        {VIEWS.map((v) => (
          <button key={v.id} className={view === v.id ? "tab active" : "tab"} onClick={() => setView(v.id)}>
            {v.label}
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
