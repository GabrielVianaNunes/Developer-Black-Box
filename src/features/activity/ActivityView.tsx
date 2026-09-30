import { useMemo, useState } from "react";
import { usePolling } from "../../app/hooks";
import { useI18n, type Key } from "../../i18n";
import { getActivity } from "../../services/backend";
import type { ActivityFilter } from "../../types/dashboard";

const PERIODS: Array<{ label: Key; ms: number | null }> = [
  { label: "activity.periodHour", ms: 3_600_000 },
  { label: "activity.periodDay", ms: 86_400_000 },
  { label: "activity.periodAll", ms: null },
];

/** Tipos de evento que o filtro oferece (o rótulo vem do dicionário: `kind.<tipo>`). */
const KINDS = [
  "ProcessStarted",
  "ProcessExited",
  "ProcessMetrics",
  "SystemMetrics",
  "AppCrash",
  "AppHang",
  "RecorderStateChanged",
  "UserMarker",
];

export function ActivityView() {
  const { t, f, kindLabel, detailText, errorText } = useI18n();
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
    <section aria-label={t("nav.activity")}>
      <div className="card filters">
        <label>
          {t("activity.type")}
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            <option value="">{t("activity.all")}</option>
            {KINDS.map((k) => (
              <option key={k} value={k}>{kindLabel(k)}</option>
            ))}
          </select>
        </label>
        <label>
          {t("activity.period")}
          <select value={period} onChange={(e) => setPeriod(Number(e.target.value))}>
            {PERIODS.map((p, i) => (
              <option key={p.label} value={i}>{t(p.label)}</option>
            ))}
          </select>
        </label>
        <label className="grow">
          {t("activity.searchApp")}
          <input
            type="search"
            value={text}
            placeholder={t("activity.searchPlaceholder")}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
        <label>
          {t("activity.show")}
          <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            {[100, 200, 500, 1000].map((n) => (
              <option key={n} value={n}>{t("activity.limitOption", { n })}</option>
            ))}
          </select>
        </label>
      </div>

      {error && <p className="notice">{errorText(error)}</p>}
      <div className="card table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t("activity.colTime")}</th>
              <th>{t("activity.colType")}</th>
              <th>{t("activity.colApp")}</th>
              <th>{t("activity.colPid")}</th>
              <th>{t("activity.colDetail")}</th>
            </tr>
          </thead>
          <tbody>
            {(rows ?? []).map((r) => (
              <tr key={r.seq}>
                <td>{f.time(r.tsUtcMs)}</td>
                <td>{kindLabel(r.kind)}</td>
                <td>{r.exeName ?? "—"}</td>
                <td>{r.pid ?? "—"}</td>
                <td>{detailText(r.detail)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && <p className="muted">{t("activity.empty")}</p>}
      </div>
    </section>
  );
}
