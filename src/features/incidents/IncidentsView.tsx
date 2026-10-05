import { useState } from "react";
import { usePolling } from "../../app/hooks";
import { useI18n } from "../../i18n";
import {
  addIncidentNote,
  captureIncident,
  deleteIncident,
  exportIncident,
  getBugReportSummary,
  getIncident,
  listIncidents,
  setIncidentState,
} from "../../services/backend";

const STATES = ["new", "investigating", "resolved", "dismissed"];

export function IncidentsView() {
  const { t, f, errorText, incidentKindLabel, severityLabel, investigationLabel } = useI18n();
  const list = usePolling(listIncidents, 3000);
  const [selected, setSelected] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  async function capture() {
    setMsg(null);
    try {
      const id = await captureIncident();
      setSelected(id);
      await list.reload();
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  return (
    <section aria-label={t("nav.incidents")}>
      <div className="row-between">
        <p className="muted">{t("incidents.intro")}</p>
        <button className="primary" onClick={capture}>{t("incidents.captureNow")}</button>
      </div>
      {msg && <p className="notice">{msg}</p>}
      {list.error && <p className="notice">{errorText(list.error)}</p>}

      <div className="split">
        <div className="card list">
          {(list.data ?? []).map((i) => (
            <button
              key={i.id}
              className={i.id === selected ? "list-item active" : "list-item"}
              onClick={() => setSelected(i.id)}
            >
              <span className={`sev sev-${i.severity}`}>{severityLabel(i.severity)}</span>
              <strong>{incidentKindLabel(i.kind)}</strong>
              <span className="muted small">
                {f.time(i.createdUtcMs)} · {investigationLabel(i.state)}
              </span>
            </button>
          ))}
          {list.data && list.data.length === 0 && <p className="muted">{t("incidents.none")}</p>}
        </div>

        <div className="card grow">
          {selected == null ? (
            <p className="muted">{t("incidents.selectOne")}</p>
          ) : (
            <IncidentDetailPanel
              key={selected}
              id={selected}
              onChanged={() => void list.reload()}
              onDeleted={() => {
                setSelected(null);
                void list.reload();
              }}
            />
          )}
        </div>
      </div>
    </section>
  );
}

function IncidentDetailPanel({ id, onChanged, onDeleted }: { id: number; onChanged: () => void; onDeleted: () => void }) {
  const { t, f, errorText, kindLabel, incidentKindLabel, severityLabel, investigationLabel, detailText, summaryText } =
    useI18n();
  const { data: d, error, reload } = usePolling(() => getIncident(id), 3000, [id]);
  const [note, setNote] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  // O texto gerado fica na tela para a pessoa conferir antes de colar em qualquer lugar.
  const [summary, setSummary] = useState<{ id: number; text: string } | null>(null);

  if (error) return <p className="notice">{errorText(error)}</p>;
  if (!d) return <p className="muted">{t("app.loading")}</p>;
  const inc = d.incident;

  async function run(fn: () => Promise<unknown>) {
    setMsg(null);
    try {
      await fn();
      await reload();
      onChanged();
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  return (
    <div>
      <h2>{incidentKindLabel(inc.kind)}</h2>
      <ul className="plain">
        <li>{t("incidents.severity", { value: severityLabel(inc.severity) })}</li>
        <li>{t("incidents.time", { value: f.time(inc.createdUtcMs) })}</li>
        <li>{t("incidents.process", { value: inc.exeName ?? "—" })}</li>
        <li>{t("incidents.summary", { value: summaryText(inc.summary) })}</li>
        <li>
          {t("incidents.capture", {
            value:
              inc.capture === "preserved"
                ? t("incidents.captureDone")
                : t("incidents.captureRunning", { time: f.time(inc.postUntilUtcMs) }),
          })}
        </li>
      </ul>

      <label className="inline">
        {t("incidents.investigation")}
        <select value={inc.state} onChange={(e) => run(() => setIncidentState(inc.id, e.target.value))}>
          {STATES.map((s) => (
            <option key={s} value={s}>{investigationLabel(s)}</option>
          ))}
        </select>
      </label>

      <h3>{t("incidents.evidence", { n: d.segments.length })}</h3>
      <ul className="plain small">
        {d.segments.map((s) => (
          <li key={s.index}>
            {t("incidents.segment", {
              index: s.index,
              size: f.bytes(s.size),
              status: s.preserved ? t("incidents.preserved") : t("incidents.notPreserved"),
            })}
          </li>
        ))}
      </ul>

      <h3>{t("incidents.timeline")}</h3>
      <div className="table-wrap tall">
        <table>
          <thead>
            <tr>
              <th>{t("incidents.colDelta")}</th>
              <th>{t("activity.colType")}</th>
              <th>{t("activity.colApp")}</th>
              <th>{t("activity.colDetail")}</th>
            </tr>
          </thead>
          <tbody>
            {d.timeline.map((r) => (
              <tr key={r.seq} className={r.offsetMs >= 0 ? "" : "before"}>
                <td>{f.offset(r.offsetMs)}</td>
                <td>{kindLabel(r.kind)}</td>
                <td>{r.exeName ?? "—"}</td>
                <td>{detailText(r.detail)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {d.timeline.length === 0 && <p className="muted">{t("incidents.timelineEmpty")}</p>}
      </div>

      <h3>{t("incidents.notes")}</h3>
      <p className="muted small">{t("incidents.notesHelp")}</p>
      <ul className="plain">
        {d.notes.map((n) => (
          <li key={n.id}><span className="muted small">{f.time(n.createdUtcMs)}</span> {n.text}</li>
        ))}
      </ul>
      <div className="row">
        <input
          className="grow"
          value={note}
          maxLength={2000}
          placeholder={t("incidents.notePlaceholder")}
          onChange={(e) => setNote(e.target.value)}
        />
        <button
          disabled={!note.trim()}
          onClick={() =>
            run(async () => {
              await addIncidentNote(inc.id, note);
              setNote("");
            })
          }
        >
          {t("incidents.add")}
        </button>
      </div>

      {msg && <p className="notice">{msg}</p>}
      <h3>{t("incidents.exportTitle")}</h3>
      <p className="muted small">{t("incidents.exportHelp")}</p>
      <button
        onClick={() =>
          run(async () => {
            const r = await exportIncident(inc.id);
            setMsg(t("incidents.exported", { path: r.path, events: r.events, dropped: r.dropped }));
          })
        }
      >
        {t("incidents.exportButton")}
      </button>
      <h3>{t("incidents.summaryTitle")}</h3>
      <p className="muted small">{t("incidents.summaryHelp")}</p>
      <button
        onClick={() =>
          run(async () => {
            const text = await getBugReportSummary(inc.id);
            setSummary({ id: inc.id, text });
            try {
              await navigator.clipboard.writeText(text);
              setMsg(t("incidents.summaryCopied"));
            } catch {
              setMsg(t("incidents.summaryNotCopied"));
            }
          })
        }
      >
        {t("incidents.summaryButton")}
      </button>
      {summary && summary.id === inc.id && (
        <textarea className="summary-text" readOnly rows={12} value={summary.text} aria-label={t("incidents.summaryTitle")} />
      )}
      <div className="row danger-zone">
        <button
          className="danger"
          onClick={async () => {
            if (window.confirm(t("incidents.confirmDelete"))) {
              await run(() => deleteIncident(inc.id));
              onDeleted();
            }
          }}
        >
          {t("incidents.delete")}
        </button>
      </div>
    </div>
  );
}
