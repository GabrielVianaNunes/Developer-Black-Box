import { useState } from "react";
import { fmtRemaining } from "../../app/format";
import { usePolling } from "../../app/hooks";
import { authorizeApp, listAuthorizations, revokeAuthorization } from "../../services/backend";

const DURATIONS = [
  { label: "15 minutos", minutes: 15 },
  { label: "30 minutos", minutes: 30 },
  { label: "1 hora", minutes: 60 },
  { label: "2 horas", minutes: 120 },
  { label: "4 horas", minutes: 240 },
  { label: "8 horas", minutes: 480 },
];

/** Modo de teste: autorização temporária, específica e revogável de um app protegido. */
export function AuthorizationsCard({ protectedApps }: { protectedApps: string[] }) {
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
      setMsg("Autorizado. A coleta começa depois da janela de estabilidade e termina sozinha no fim do prazo.");
    } catch (e) {
      setMsg(typeof e === "string" ? e : "Não foi possível autorizar.");
    }
  }

  async function revoke(name: string) {
    setMsg(null);
    try {
      await revokeAuthorization(name);
      await active.reload();
      setMsg("Autorização revogada; a coleta desse aplicativo parou agora.");
    } catch {
      setMsg("Não foi possível revogar.");
    }
  }

  return (
    <div className="card">
      <h2>Aplicações autorizadas para testes</h2>
      <p className="muted">
        Para testar uma aplicação web que você desenvolve, autorize por um tempo limitado a coleta{" "}
        <strong>técnica</strong> de um navegador. Nunca há conteúdo de páginas, formulários, senhas, cookies, URLs nem
        requisições: o registro só tem processo, CPU, memória e falhas.
      </p>
      <ul className="plain small muted">
        <li>Vale só para o aplicativo escolhido e expira sozinha; não sobrevive ao fechar o app.</li>
        <li>
          Com o navegador autorizado em primeiro plano, a gravação continua em modo restrito: só ele é gravado, nada
          dos outros aplicativos e nem métricas do sistema.
        </li>
        <li>Gerenciadores de senha e sessão bloqueada continuam bloqueando a gravação.</li>
        <li>Uma regra de exclusão vence qualquer autorização.</li>
      </ul>

      <div className="filters">
        <label>
          Aplicativo (protegido)
          <select value={chosen} onChange={(e) => setExe(e.target.value)}>
            {protectedApps.map((a) => (
              <option key={a} value={a}>{a}</option>
            ))}
          </select>
        </label>
        <label>
          Duração
          <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
            {DURATIONS.map((d) => (
              <option key={d.minutes} value={d.minutes}>{d.label}</option>
            ))}
          </select>
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={metrics} onChange={(e) => setMetrics(e.target.checked)} />
        Métricas dos processos (início, fim, CPU e memória)
      </label>
      <label className="check">
        <input type="checkbox" checked={crashes} onChange={(e) => setCrashes(e.target.checked)} />
        Falhas e travamentos registrados pelo Windows
      </label>
      <div className="row">
        <button className="primary" disabled={!chosen || (!metrics && !crashes)} onClick={authorize}>
          Autorizar
        </button>
      </div>
      {msg && <p className="notice" role="status">{msg}</p>}

      <h3>Autorizações ativas</h3>
      <ul className="plain">
        {(active.data ?? []).map((a) => (
          <li key={a.exe} className="row-between">
            <span>
              <strong>{a.exe}</strong> · restam {fmtRemaining(a.remainingMs)} ·{" "}
              {[a.allowMetrics && "métricas", a.allowCrashes && "falhas"].filter(Boolean).join(" e ")}
            </span>
            <button className="danger" onClick={() => revoke(a.exe)}>Revogar</button>
          </li>
        ))}
      </ul>
      {active.data && active.data.length === 0 && <p className="muted">Nenhuma autorização ativa.</p>}
    </div>
  );
}
