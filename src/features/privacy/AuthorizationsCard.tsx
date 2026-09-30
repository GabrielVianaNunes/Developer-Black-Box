import { useState } from "react";
import { usePolling } from "../../app/hooks";
import { useI18n, type Key } from "../../i18n";
import { authorizeApp, listAuthorizations, revokeAuthorization } from "../../services/backend";

const DURATIONS: Array<{ label: Key; minutes: number }> = [
  { label: "auth.d15", minutes: 15 },
  { label: "auth.d30", minutes: 30 },
  { label: "auth.d60", minutes: 60 },
  { label: "auth.d120", minutes: 120 },
  { label: "auth.d240", minutes: 240 },
  { label: "auth.d480", minutes: 480 },
];

/** Modo de teste: autorização temporária, específica e revogável de um app protegido. */
export function AuthorizationsCard({ protectedApps }: { protectedApps: string[] }) {
  const { t, f, errorText } = useI18n();
  const active = usePolling(listAuthorizations, 1000);
  const [exe, setExe] = useState("");
  const [minutes, setMinutes] = useState(30);
  const [metrics, setMetrics] = useState(true);
  const [crashes, setCrashes] = useState(true);
  const [msg, setMsg] = useState<string | null>(null);

  const chosen = exe || protectedApps[0] || "";

  async function authorize() {
    setMsg(null);
    try {
      await authorizeApp(chosen, minutes, metrics, crashes);
      await active.reload();
      setMsg(t("auth.authorized"));
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  async function revoke(name: string) {
    setMsg(null);
    try {
      await revokeAuthorization(name);
      await active.reload();
      setMsg(t("auth.revoked"));
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  return (
    <div className="card">
      <h2>{t("auth.title")}</h2>
      <p className="muted">{t("auth.intro")}</p>
      <ul className="plain small muted">
        <li>{t("auth.point1")}</li>
        <li>{t("auth.point2")}</li>
        <li>{t("auth.point3")}</li>
        <li>{t("auth.point4")}</li>
      </ul>

      <div className="filters">
        <label>
          {t("auth.app")}
          <select value={chosen} onChange={(e) => setExe(e.target.value)}>
            {protectedApps.map((a) => (
              <option key={a} value={a}>{a}</option>
            ))}
          </select>
        </label>
        <label>
          {t("auth.duration")}
          <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
            {DURATIONS.map((d) => (
              <option key={d.minutes} value={d.minutes}>{t(d.label)}</option>
            ))}
          </select>
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={metrics} onChange={(e) => setMetrics(e.target.checked)} />
        {t("auth.metrics")}
      </label>
      <label className="check">
        <input type="checkbox" checked={crashes} onChange={(e) => setCrashes(e.target.checked)} />
        {t("auth.crashes")}
      </label>
      <div className="row">
        <button className="primary" disabled={!chosen || (!metrics && !crashes)} onClick={authorize}>
          {t("auth.authorize")}
        </button>
      </div>
      {msg && <p className="notice" role="status">{msg}</p>}

      <h3>{t("auth.active")}</h3>
      <ul className="plain">
        {(active.data ?? []).map((a) => {
          const parts = [a.allowMetrics && t("auth.sourceMetrics"), a.allowCrashes && t("auth.sourceCrashes")].filter(
            (x): x is string => typeof x === "string",
          );
          const sources = parts.length === 2 ? t("auth.sourcesJoin", { a: parts[0], b: parts[1] }) : parts.join("");
          return (
            <li key={a.exe} className="row-between">
              <span>
                {t("auth.activeEntry", { name: a.exe, remaining: f.remaining(a.remainingMs), sources })}
              </span>
              <button className="danger" onClick={() => revoke(a.exe)}>{t("auth.revoke")}</button>
            </li>
          );
        })}
      </ul>
      {active.data && active.data.length === 0 && <p className="muted">{t("auth.noneActive")}</p>}
    </div>
  );
}
