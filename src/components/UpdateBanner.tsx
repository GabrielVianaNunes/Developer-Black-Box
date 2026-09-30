import { useI18n } from "../i18n";
import { openReleasePage } from "../services/backend";
import type { UpdateState } from "../types/update";

interface Props {
  state: UpdateState | null;
  download: () => void;
  install: () => void;
  installing: boolean;
  actionError: string | null;
}

/** Aviso discreto no topo quando existe versão nova. Só age quando você clica. */
export function UpdateBanner({ state, download, install, installing, actionError }: Props) {
  const { t, errorText } = useI18n();
  if (!state?.available || !state.latest) return null;
  const error = actionError ?? (state.error ? errorText(state.error) : null);
  return (
    <div className="update-banner" role="status">
      <span>{state.ready ? t("update.ready", { version: state.latest }) : t("update.banner", { version: state.latest })}</span>
      {state.ready ? (
        <button className="primary" disabled={installing} onClick={install}>
          {installing ? t("update.installing") : t("update.install")}
        </button>
      ) : (
        <button className="primary" disabled={state.downloading} onClick={download}>
          {state.downloading ? t("update.downloading") : t("update.download")}
        </button>
      )}
      <button className="link" onClick={() => openReleasePage().catch(() => {})}>
        {t("update.openPage")}
      </button>
      {error && <span className="notice">{error}</span>}
    </div>
  );
}
