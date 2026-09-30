import { useMemo, useState } from "react";
import { KIND_LABEL, fmtTime } from "../../app/format";
import { usePolling } from "../../app/hooks";
import { getActivity } from "../../services/backend";
import type { ActivityFilter } from "../../types/dashboard";

const PERIODS: Array<{ label: string; ms: number | null }> = [
  { label: "Última hora", ms: 3_600_000 },
  { label: "Últimas 24 h", ms: 86_400_000 },
  { label: "Tudo o que foi gravado", ms: null },
];

export function ActivityView() {
  const [kind, setKind] = useState("");
  const [text, setText] = useState("");
  const [period, setPeriod] = useState(2);
  const [limit, setLimit] = useState(200);

  const filter = useMemo<ActivityFilter>(() => {
    const ms = PERIODS[period].ms;
    return {
      kinds: kind ? [kind] : [],
      text,
      fromUtcMs: ms ? Date.now() - ms : null,
      toUtcMs: null,
      limit,
    };
  }, [kind, text, period, limit]);

  const { data: rows, error } = usePolling(() => getActivity(filter), 3000, [filter]);

  return (
    <section aria-label="Atividade">
      <div className="card filters">
        <label>
          Tipo
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            <option value="">Todos</option>
            {Object.entries(KIND_LABEL).map(([k, v]) => (
              <option key={k} value={k}>{v}</option>
            ))}
          </select>
        </label>
        <label>
          Período
          <select value={period} onChange={(e) => setPeriod(Number(e.target.value))}>
            {PERIODS.map((p, i) => (
              <option key={p.label} value={i}>{p.label}</option>
            ))}
          </select>
        </label>
        <label className="grow">
          Buscar por aplicativo
          <input type="search" value={text} placeholder="ex.: code.exe" onChange={(e) => setText(e.target.value)} />
        </label>
        <label>
          Mostrar
          <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            {[100, 200, 500, 1000].map((n) => (
              <option key={n} value={n}>{n} eventos</option>
            ))}
          </select>
        </label>
      </div>

      {error && <p className="notice">{error}</p>}
      <div className="card table-wrap">
        <table>
          <thead>
            <tr><th>Horário</th><th>Tipo</th><th>Aplicativo</th><th>PID</th><th>Detalhe</th></tr>
          </thead>
          <tbody>
            {(rows ?? []).map((r) => (
              <tr key={r.seq}>
                <td>{fmtTime(r.tsUtcMs)}</td>
                <td>{KIND_LABEL[r.kind] ?? r.kind}</td>
                <td>{r.exeName ?? "—"}</td>
                <td>{r.pid ?? "—"}</td>
                <td>{r.detail}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && (
          <p className="muted">Nenhum evento para este filtro. Com a gravação pausada ou suspensa, nada é registrado.</p>
        )}
      </div>
    </section>
  );
}
