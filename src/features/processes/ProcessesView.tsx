import { usePolling } from "../../app/hooks";
import { useI18n } from "../../i18n";
import { getProcesses } from "../../services/backend";

export function ProcessesView() {
  const { t, f, errorText } = useI18n();
  const { data: rows, error } = usePolling(getProcesses, 3000);

  return (
    <section aria-label={t("nav.processes")}>
      <p className="muted">{t("processes.note")}</p>
      {error && <p className="notice">{errorText(error)}</p>}
      <div className="card table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t("processes.colApp")}</th>
              <th>{t("processes.colPid")}</th>
              <th>{t("processes.colStatus")}</th>
              <th>{t("processes.colCpu")}</th>
              <th>{t("processes.colMemory")}</th>
              <th>{t("processes.colStarted")}</th>
              <th>{t("processes.colEnded")}</th>
            </tr>
          </thead>
          <tbody>
            {(rows ?? []).map((p) => (
              <tr key={`${p.pid}-${p.startTimeMs}`}>
                <td>{p.exeName}</td>
                <td>{p.pid}</td>
                <td>{p.running ? t("processes.running") : t("processes.exited")}</td>
                <td>{p.cpuPermille != null ? f.pct(p.cpuPermille) : "—"}</td>
                <td>{p.workingSetKb != null ? f.kb(p.workingSetKb) : "—"}</td>
                <td>{f.time(p.startTimeMs)}</td>
                <td>{p.endedUtcMs ? f.time(p.endedUtcMs) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && <p className="muted">{t("processes.empty")}</p>}
      </div>
    </section>
  );
}
