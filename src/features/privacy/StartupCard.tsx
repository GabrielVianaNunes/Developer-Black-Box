import { useEffect, useState } from "react";
import { getLaunchAtLogin, setLaunchAtLogin } from "../../services/backend";

/** Abrir com o Windows: só registra o app para abrir escondido na bandeja; não inicia a gravação. */
export function StartupCard() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    getLaunchAtLogin().then(setEnabled).catch(() => setEnabled(false));
  }, []);

  async function toggle(next: boolean) {
    setMsg(null);
    try {
      setEnabled(await setLaunchAtLogin(next));
      setMsg(next ? "O app vai abrir na bandeja quando você entrar no Windows." : "O app não abre mais com o Windows.");
    } catch (e) {
      setMsg(typeof e === "string" ? e : "Não foi possível alterar.");
    }
  }

  return (
    <div className="card">
      <h2>Inicialização</h2>
      <label className="check">
        <input type="checkbox" disabled={enabled === null} checked={!!enabled} onChange={(e) => toggle(e.target.checked)} />
        Abrir com o Windows
      </label>
      <p className="muted small">
        O app abre escondido, só com o ícone na bandeja, e <strong>continua pausado</strong>: abrir com o Windows não
        começa a gravar. Só grava sozinho se você também ligar "Começar a gravar ao abrir o aplicativo" acima. Usa
        apenas o seu usuário (sem administrador) e o instalador remove a entrada ao desinstalar.
      </p>
      {msg && <p className="notice" role="status">{msg}</p>}
    </div>
  );
}
