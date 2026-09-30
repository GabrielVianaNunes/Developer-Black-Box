import { fmtBytes, fmtClock, fmtKb, fmtPct } from "../../app/format";
import { usePolling } from "../../app/hooks";
import { getOverview } from "../../services/backend";
import type { Status } from "../../types/status";

export function OverviewView({ status }: { status: Status }) {
  const { data: o, error } = usePolling(getOverview, 3000);

  return (
    <section aria-label="Visão geral">
      <div className="grid">
        <Stat label="Estado" value={status.text} />
        <Stat label="Armazenamento usado" value={o ? fmtBytes(o.storageBytes) : "…"} hint={o ? `${o.sealedSegments} segmentos selados` : ""} />
        <Stat
          label="Incidentes"
          value={o ? String(o.incidentsTotal) : "…"}
          hint={o ? `${o.incidentsOpen} em aberto` : ""}
        />
        <Stat label="Eventos na última hora" value={o ? String(o.eventsLastHour) : "…"} />
      </div>

      <div className="card">
        <h2>Atividade técnica (última hora)</h2>
        {o ? (
          <ul className="plain">
            <li>Processos iniciados: {o.startsLastHour}</li>
            <li>Processos encerrados: {o.exitsLastHour}</li>
            <li>
              Sistema:{" "}
              {o.latestSystem
                ? `CPU ${fmtPct(o.latestSystem.cpuPermille)} · memória ${fmtKb(o.latestSystem.memUsedKb)} de ${fmtKb(
                    o.latestSystem.memTotalKb,
                  )} (às ${fmtClock(o.latestSystem.tsUtcMs)})`
                : "sem métricas gravadas ainda"}
            </li>
          </ul>
        ) : (
          <p className="muted">Carregando…</p>
        )}
        <p className="muted">
          Só aparece o que foi gravado com o Privacy Guard ativo. Nada é reconstruído para os períodos de pausa ou
          bloqueio.
        </p>
        {error && <p className="notice">{error}</p>}
      </div>
    </section>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {hint ? <div className="muted small">{hint}</div> : null}
    </div>
  );
}
