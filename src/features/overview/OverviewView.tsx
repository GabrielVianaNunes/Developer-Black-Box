import { usePolling } from "../../app/hooks";
import { useI18n } from "../../i18n";
import { getOverview } from "../../services/backend";
import type { Status } from "../../types/status";

export function OverviewView({ status }: { status: Status }) {
  const { t, f, errorText } = useI18n();
  const { data: o, error } = usePolling(getOverview, 3000);

  return (
    <section aria-label={t("nav.overview")}>
      <div className="grid">
        <Stat label={t("overview.state")} value={status.text} />
        <Stat
          label={t("overview.storageUsed")}
          value={o ? f.bytes(o.storageBytes) : "…"}
          hint={o ? t("overview.segmentsSealed", { n: o.sealedSegments }) : ""}
        />
        <Stat
          label={t("overview.incidents")}
          value={o ? String(o.incidentsTotal) : "…"}
          hint={o ? t("overview.incidentsOpen", { n: o.incidentsOpen }) : ""}
        />
        <Stat label={t("overview.eventsLastHour")} value={o ? String(o.eventsLastHour) : "…"} />
      </div>

      <div className="card">
        <h2>{t("overview.activityTitle")}</h2>
        {o ? (
          <ul className="plain">
            <li>{t("overview.started", { n: o.startsLastHour })}</li>
            <li>{t("overview.exited", { n: o.exitsLastHour })}</li>
            <li>
              {o.latestSystem
                ? t("overview.system", {
                    cpu: f.pct(o.latestSystem.cpuPermille),
                    used: f.kb(o.latestSystem.memUsedKb),
                    total: f.kb(o.latestSystem.memTotalKb),
                    time: f.clock(o.latestSystem.tsUtcMs),
                  })
                : t("overview.noSystem")}
            </li>
          </ul>
        ) : (
          <p className="muted">{t("app.loading")}</p>
        )}
        <p className="muted">{t("overview.footnote")}</p>
        {error && <p className="notice">{errorText(error)}</p>}
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
