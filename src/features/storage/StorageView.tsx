import { useEffect, useState } from "react";
import { fmtBytes } from "../../app/format";
import { usePolling } from "../../app/hooks";
import { deleteActivity, getSettings, getStorage, setSettings, verifyIntegrity } from "../../services/backend";
import type { Settings, Verify } from "../../types/dashboard";

export function StorageView() {
  const st = usePolling(getStorage, 4000);
  const [settings, setLocal] = useState<Settings | null>(null);
  const [mb, setMb] = useState(256);
  const [hours, setHours] = useState(24);
  const [verify, setVerify] = useState<Verify | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    getSettings().then((s) => {
      setLocal(s);
      setMb(s.retentionMaxMb);
      setHours(s.retentionMaxHours);
    });
  }, []);

  const s = st.data;
  const used = s ? Math.min(100, (s.storageBytes / Math.max(1, s.maxTotalBytes)) * 100) : 0;
  const preserved = s ? s.segments.filter((x) => x.preserved).length : 0;

  async function saveRetention() {
    if (!settings) return;
    setMsg(null);
    try {
      const next = await setSettings({ ...settings, retentionMaxMb: mb, retentionMaxHours: hours });
      setLocal(next);
      setMsg("Retenção aplicada. Segmentos acima do novo limite foram removidos, exceto os preservados.");
      void st.reload();
    } catch (e) {
      setMsg(typeof e === "string" ? e : "Não foi possível salvar.");
    }
  }

  async function remove(includePreserved: boolean) {
    const q = includePreserved
      ? "Excluir TODA a atividade gravada, incluindo as evidências de incidentes? Os incidentes também serão removidos. Não dá para desfazer."
      : "Excluir a atividade gravada? As evidências preservadas de incidentes são mantidas. Não dá para desfazer.";
    if (!window.confirm(q)) return;
    setMsg(null);
    try {
      const n = await deleteActivity(includePreserved);
      setMsg(`${n} segmento(s) excluído(s).`);
      void st.reload();
    } catch (e) {
      setMsg(typeof e === "string" ? e : "Não foi possível excluir.");
    }
  }

  return (
    <section aria-label="Armazenamento">
      <div className="card">
        <h2>Espaço utilizado</h2>
        {s ? (
          <>
            <div className="bar" role="progressbar" aria-valuenow={Math.round(used)} aria-valuemin={0} aria-valuemax={100}>
              <div className="bar-fill" style={{ width: `${used}%` }} />
            </div>
            <p>
              {fmtBytes(s.storageBytes)} de {fmtBytes(s.maxTotalBytes)} · {s.segments.length} segmentos ({preserved}{" "}
              preservados como evidência)
            </p>
          </>
        ) : (
          <p className="muted">Carregando…</p>
        )}
        <p className="muted small">Os dados ficam em %LOCALAPPDATA%\DeveloperBlackBox, cifrados, fora do repositório.</p>
      </div>

      <div className="card">
        <h2>Retenção</h2>
        <div className="row">
          <label className="inline">
            Limite (MB)
            <input type="number" min={16} max={10240} value={mb} onChange={(e) => setMb(Number(e.target.value))} />
          </label>
          <label className="inline">
            Guardar por (horas)
            <input type="number" min={1} max={720} value={hours} onChange={(e) => setHours(Number(e.target.value))} />
          </label>
          <button className="primary" onClick={saveRetention}>Aplicar</button>
        </div>
        <p className="muted small">
          Ao reduzir o limite, os segmentos mais antigos que não sejam evidência são removidos na hora.
        </p>
      </div>

      <div className="card">
        <h2>Integridade</h2>
        <button onClick={async () => setVerify(await verifyIntegrity())}>Verificar integridade</button>
        {verify && (
          <p role="status" className={verify.ok ? "ok" : "bad"}>
            {verify.ok
              ? `Íntegro: ${verify.segments} segmentos, ${verify.events} eventos autenticados.`
              : `Problema encontrado: ${verify.error}`}
          </p>
        )}
        <p className="muted small">Confere a autenticação de cada segmento e a cadeia de hashes entre eles.</p>
      </div>

      <div className="card">
        <h2>Exclusão de dados</h2>
        <div className="row">
          <button className="danger" onClick={() => remove(false)}>Excluir atividade</button>
          <button className="danger" onClick={() => remove(true)}>Excluir tudo (inclui evidências)</button>
        </div>
        <p className="muted small">
          A exportação de evidências fica no detalhe de cada incidente (seção Incidentes) e aplica de novo as regras de
          privacidade de agora.
        </p>
        {msg && <p className="notice" role="status">{msg}</p>}
        {st.error && <p className="notice">{st.error}</p>}
      </div>
    </section>
  );
}
