import { useMemo } from "react";
import { usePolling } from "../../app/hooks";
import { useI18n, type Key } from "../../i18n";
import { getActivity, getHealthStatus } from "../../services/backend";
import type { ActivityFilter } from "../../types/dashboard";
import { NEVER_RECORDED, NOT_MONITORED, SAMPLE_KIND, TIMELINE_KINDS, orderedSources, splitTimeline, stateTone } from "./model";

const WEEK_MS = 7 * 24 * 3_600_000;

export function HealthView() {
  const { t, f, errorText, kindLabel, detailText } = useI18n();
  const status = usePolling(getHealthStatus, 3000);
  const timelineFilter = useMemo<ActivityFilter>(
    () => ({ kinds: [...TIMELINE_KINDS], text: "", fromUtcMs: Date.now() - WEEK_MS, toUtcMs: null, limit: 300 }),
    [],
  );
  const timeline = usePolling(() => getActivity(timelineFilter), 5000, [timelineFilter]);
  const sample = usePolling(
    () => getActivity({ kinds: [SAMPLE_KIND], text: "", fromUtcMs: null, toUtcMs: null, limit: 1 }),
    5000,
  );

  const sources = orderedSources(status.data ?? []);
  const { timeline: events, inventory } = splitTimeline(timeline.data ?? []);
  const latest = sample.data?.[0];
  const error = status.error ?? timeline.error ?? sample.error;

  return (
    <section aria-label={t("nav.health")}>
      <div className="card">
        <h2>{t("health.sourcesTitle")}</h2>
        <p className="muted small">{t("health.sourcesHelp")}</p>
        <table>
          <tbody>
            {sources.map((s) => (
              <tr key={s.source}>
                <td><strong>{t(`health.source.${s.source}` as Key)}</strong></td>
                <td>
                  <span className={`badge health-${stateTone(s.state)}`}>{t(`health.state.${s.state}` as Key)}</span>
                </td>
                <td className="muted small">{t(`health.sourceHelp.${s.source}` as Key)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {status.data && sources.length === 0 && <p className="muted">{t("health.noSources")}</p>}
      </div>

      <div className="card">
        <h2>{t("health.latestTitle")}</h2>
        {latest ? (
          <p>
            {detailText(latest.detail)} <span className="muted small">· {f.time(latest.tsUtcMs)}</span>
          </p>
        ) : (
          <p className="muted">{t("health.noSample")}</p>
        )}
      </div>

      <div className="card table-wrap">
        <h2>{t("health.timelineTitle")}</h2>
        <p className="muted small">{t("health.timelineHelp")}</p>
        <table>
          <thead>
            <tr>
              <th>{t("activity.colTime")}</th>
              <th>{t("activity.colType")}</th>
              <th>{t("activity.colDetail")}</th>
            </tr>
          </thead>
          <tbody>
            {events.map((r) => (
              <tr key={r.seq}>
                <td>{f.time(r.tsUtcMs)}</td>
                <td>{kindLabel(r.kind)}</td>
                <td>{detailText(r.detail)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {timeline.data && events.length === 0 && <p className="muted">{t("health.timelineEmpty")}</p>}
      </div>

      <div className="card table-wrap">
        <h2>{t("health.inventoryTitle")}</h2>
        <p className="muted small">{t("health.inventoryHelp")}</p>
        <table>
          <tbody>
            {inventory.map((r) => (
              <tr key={r.seq}>
                <td>{f.time(r.tsUtcMs)}</td>
                <td>{detailText(r.detail)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {timeline.data && inventory.length === 0 && <p className="muted">{t("health.inventoryEmpty")}</p>}
      </div>

      <div className="card">
        <h2>{t("health.notMonitoredTitle")}</h2>
        <p className="muted small">{t("health.notMonitoredHelp")}</p>
        <ul className="plain">
          {NOT_MONITORED.map((k) => (
            <li key={k}>{t(`health.not.${k}` as Key)}</li>
          ))}
        </ul>
        <h2>{t("health.neverTitle")}</h2>
        <ul className="plain">
          {NEVER_RECORDED.map((k) => (
            <li key={k}>{t(`health.never.${k}` as Key)}</li>
          ))}
        </ul>
      </div>

      {error && <p className="notice">{errorText(error)}</p>}
    </section>
  );
}
