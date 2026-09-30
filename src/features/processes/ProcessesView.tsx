import { fmtKb, fmtPct, fmtTime } from "../../app/format";
import { usePolling } from "../../app/hooks";
import { getProcesses } from "../../services/backend";

export function ProcessesView() {
  const { data: rows, error } = usePolling(getProcesses, 3000);

  return (
    <section aria-label="Processos">
      <p className="muted">
        Instâncias vistas nos registros. "Em execução" segue o último registro: com a gravação pausada esse estado pode
        estar desatualizado. Aplicativos protegidos ou excluídos não aparecem.
      </p>
      {error && <p className="notice">{error}</p>}
      <div className="card table-wrap">
        <table>
          <thead>
            <tr><th>Aplicativo</th><th>PID</th><th>Situação</th><th>CPU</th><th>Memória</th><th>Iniciado</th><th>Encerrado</th></tr>
          </thead>
          <tbody>
            {(rows ?? []).map((p) => (
              <tr key={`${p.pid}-${p.startTimeMs}`}>
                <td>{p.exeName}</td>
                <td>{p.pid}</td>
                <td>{p.running ? "Em execução" : "Encerrado"}</td>
                <td>{p.cpuPermille != null ? fmtPct(p.cpuPermille) : "—"}</td>
                <td>{p.workingSetKb != null ? fmtKb(p.workingSetKb) : "—"}</td>
                <td>{fmtTime(p.startTimeMs)}</td>
                <td>{p.endedUtcMs ? fmtTime(p.endedUtcMs) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && <p className="muted">Nenhum processo registrado ainda.</p>}
      </div>
    </section>
  );
}
