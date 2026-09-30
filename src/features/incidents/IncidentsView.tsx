import { useState } from "react";
import {
  INCIDENT_KIND_LABEL,
  INVESTIGATION_LABEL,
  KIND_LABEL,
  SEVERITY_LABEL,
  fmtBytes,
  fmtOffset,
  fmtTime,
} from "../../app/format";
import { usePolling } from "../../app/hooks";
import {
  addIncidentNote,
  captureIncident,
  deleteIncident,
  exportIncident,
  getIncident,
  listIncidents,
  setIncidentState,
} from "../../services/backend";

export function IncidentsView() {
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
      setMsg(typeof e === "string" ? e : "Não foi possível capturar.");
    }
  }

  return (
    <section aria-label="Incidentes">
      <div className="row-between">
        <p className="muted">
          Um incidente preserva evidências (janela anterior e posterior) e registra fatos. Ele não afirma causa: eventos
          próximos no tempo não significam relação causal.
        </p>
        <button className="primary" onClick={capture}>Capturar agora</button>
      </div>
      {msg && <p className="notice">{msg}</p>}
      {list.error && <p className="notice">{list.error}</p>}

      <div className="split">
        <div className="card list">
          {(list.data ?? []).map((i) => (
            <button
              key={i.id}
              className={i.id === selected ? "list-item active" : "list-item"}
              onClick={() => setSelected(i.id)}
            >
              <span className={`sev sev-${i.severity}`}>{SEVERITY_LABEL[i.severity] ?? i.severity}</span>
              <strong>{INCIDENT_KIND_LABEL[i.kind] ?? i.kind}</strong>
              <span className="muted small">
                {fmtTime(i.createdUtcMs)} · {INVESTIGATION_LABEL[i.state] ?? i.state}
              </span>
            </button>
          ))}
          {list.data && list.data.length === 0 && <p className="muted">Nenhum incidente.</p>}
        </div>

        <div className="card grow">
          {selected == null ? (
            <p className="muted">Selecione um incidente para ver os detalhes.</p>
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
  const { data: d, error, reload } = usePolling(() => getIncident(id), 3000, [id]);
  const [note, setNote] = useState("");
  const [msg, setMsg] = useState<string | null>(null);

  if (error) return <p className="notice">{error}</p>;
  if (!d) return <p className="muted">Carregando…</p>;
  const inc = d.incident;

  async function run(fn: () => Promise<unknown>) {
    setMsg(null);
    try {
      await fn();
      await reload();
      onChanged();
    } catch (e) {
      setMsg(typeof e === "string" ? e : "A ação não pôde ser concluída.");
    }
  }

  return (
    <div>
      <h2>{INCIDENT_KIND_LABEL[inc.kind] ?? inc.kind}</h2>
      <ul className="plain">
        <li>Gravidade: {SEVERITY_LABEL[inc.severity] ?? inc.severity}</li>
        <li>Horário: {fmtTime(inc.createdUtcMs)}</li>
        <li>Processo relacionado: {inc.exeName ?? "—"}</li>
        <li>Resumo técnico: {inc.summary}</li>
        <li>
          Captura: {inc.capture === "preserved" ? "concluída" : `em andamento (até ${fmtTime(inc.postUntilUtcMs)})`}
        </li>
      </ul>

      <label className="inline">
        Investigação
        <select value={inc.state} onChange={(e) => run(() => setIncidentState(inc.id, e.target.value))}>
          {Object.entries(INVESTIGATION_LABEL).map(([k, v]) => (
            <option key={k} value={k}>{v}</option>
          ))}
        </select>
      </label>

      <h3>Evidências ({d.segments.length} segmentos)</h3>
      <ul className="plain small">
        {d.segments.map((s) => (
          <li key={s.index}>Segmento {s.index} · {fmtBytes(s.size)} · {s.preserved ? "preservado" : "não preservado"}</li>
        ))}
      </ul>

      <h3>Linha do tempo</h3>
      <div className="table-wrap tall">
        <table>
          <thead><tr><th>Δ</th><th>Tipo</th><th>Aplicativo</th><th>Detalhe</th></tr></thead>
          <tbody>
            {d.timeline.map((r) => (
              <tr key={r.seq} className={r.offsetMs >= 0 ? "" : "before"}>
                <td>{fmtOffset(r.offsetMs)}</td>
                <td>{KIND_LABEL[r.kind] ?? r.kind}</td>
                <td>{r.exeName ?? "—"}</td>
                <td>{r.detail}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {d.timeline.length === 0 && <p className="muted">Sem eventos nos segmentos de evidência.</p>}
      </div>

      <h3>Anotações</h3>
      <p className="muted small">
        Texto seu, guardado localmente sem criptografia. Não escreva senhas nem dados pessoais.
      </p>
      <ul className="plain">
        {d.notes.map((n) => (
          <li key={n.id}><span className="muted small">{fmtTime(n.createdUtcMs)}</span> {n.text}</li>
        ))}
      </ul>
      <div className="row">
        <input
          className="grow"
          value={note}
          maxLength={2000}
          placeholder="Adicionar anotação"
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
          Adicionar
        </button>
      </div>

      {msg && <p className="notice">{msg}</p>}
      <h3>Exportar evidências</h3>
      <p className="muted small">
        As regras de privacidade de agora são aplicadas de novo: aplicativos excluídos ou protegidos depois da gravação
        saem do arquivo, e eventos de origem desconhecida são descartados. Anotações não são exportadas. O arquivo
        exportado <strong>não é cifrado</strong>.
      </p>
      <button
        onClick={() =>
          run(async () => {
            const r = await exportIncident(inc.id);
            setMsg(`Exportado em ${r.path} (${r.events} eventos; ${r.dropped} removidos pela filtragem).`);
          })
        }
      >
        Exportar incidente
      </button>
      <div className="row danger-zone">
        <button
          className="danger"
          onClick={async () => {
            if (window.confirm("Excluir este incidente e as anotações? As evidências deixam de ser preservadas.")) {
              await run(() => deleteIncident(inc.id));
              onDeleted();
            }
          }}
        >
          Excluir incidente
        </button>
      </div>
    </div>
  );
}
