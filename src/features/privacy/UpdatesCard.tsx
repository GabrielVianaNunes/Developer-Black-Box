import { useState } from "react";
import { useUpdate } from "../../app/useUpdate";
import { useI18n } from "../../i18n";
import { checkForUpdates, openReleasePage, setUpdateCheck } from "../../services/backend";

/** Atualizações: a verificação automática vem desligada; "Verificar agora" é sempre uma ação sua. */
export function UpdatesCard() {
  const { t, f, errorText } = useI18n();
  const { state, setState, download, install, installing, actionError } = useUpdate();
  const [msg, setMsg] = useState<string | null>(null);

  async function toggle(next: boolean) {
    setMsg(null);
    try {
      setState(await setUpdateCheck(next));
      setMsg(next ? t("update.on") : t("update.off"));
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  async function checkNow() {
    setMsg(null);
    try {
      setState(await checkForUpdates());
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  const result = !state
    ? null
    : state.checking
      ? t("update.checking")
      : state.error
        ? errorText(state.error)
        : state.available && state.latest && state.ready
          ? t("update.ready", { version: state.latest })
          : state.available && state.latest
          ? t("update.available", { version: state.latest })
          : state.checkedUtcMs
            ? t("update.upToDate")
            : t("update.never");

  return (
    <div className="card">
      <h2>{t("update.title")}</h2>
      <label className="check">
        <input type="checkbox" disabled={!state} checked={!!state?.enabled} onChange={(e) => toggle(e.target.checked)} />
        {t("update.checkbox")}
      </label>
      <p className="muted small">{t("update.help")}</p>
      {state && <p className="small">{t("update.current", { version: state.current })}</p>}
      <div className="actions">
        <button disabled={!state || state.checking} onClick={checkNow}>
          {state?.checking ? t("update.checking") : t("update.checkNow")}
        </button>
        {state?.available && !state.ready && (
          <button className="primary" disabled={state.downloading} onClick={download}>
            {state.downloading ? t("update.downloading") : t("update.download")}
          </button>
        )}
        {state?.available && state.ready && (
          <button className="primary" disabled={installing} onClick={install}>
            {installing ? t("update.installing") : t("update.install")}
          </button>
        )}
        {state?.available && (
          <button onClick={() => openReleasePage().catch((e) => setMsg(errorText(e)))}>{t("update.openPage")}</button>
        )}
      </div>
      {result && <p className="notice" role="status">{result}</p>}
      {state?.checkedUtcMs && <p className="muted small">{t("update.lastChecked", { time: f.time(state.checkedUtcMs) })}</p>}
      {state?.available && <p className="muted small">{t("update.installNote")}</p>}
      {actionError && <p className="notice" role="status">{actionError}</p>}
      {msg && <p className="notice" role="status">{msg}</p>}
    </div>
  );
}
