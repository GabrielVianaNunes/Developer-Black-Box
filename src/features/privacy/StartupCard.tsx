import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { getLaunchAtLogin, setLaunchAtLogin } from "../../services/backend";

/** Abrir com o Windows: só registra o app para abrir escondido na bandeja; não inicia a gravação. */
export function StartupCard() {
  const { t, errorText } = useI18n();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    getLaunchAtLogin().then(setEnabled).catch(() => setEnabled(false));
  }, []);

  async function toggle(next: boolean) {
    setMsg(null);
    try {
      setEnabled(await setLaunchAtLogin(next));
      setMsg(next ? t("startup.on") : t("startup.off"));
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  return (
    <div className="card">
      <h2>{t("startup.title")}</h2>
      <label className="check">
        <input type="checkbox" disabled={enabled === null} checked={!!enabled} onChange={(e) => toggle(e.target.checked)} />
        {t("startup.checkbox")}
      </label>
      <p className="muted small">{t("startup.help")}</p>
      {msg && <p className="notice" role="status">{msg}</p>}
    </div>
  );
}
