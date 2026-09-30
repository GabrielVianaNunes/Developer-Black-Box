import { useI18n } from "../i18n";
import { openReleasePage } from "../services/backend";
import type { UpdateState } from "../types/update";

/** Aviso discreto no topo quando existe versão nova. Só informa; não baixa nem instala nada. */
export function UpdateBanner({ state }: { state: UpdateState | null }) {
  const { t, errorText } = useI18n();
  if (!state?.available || !state.latest) return null;
  return (
    <div className="update-banner" role="status">
      <span>{t("update.banner", { version: state.latest })}</span>
      <button className="link" onClick={() => openReleasePage().catch((e) => window.alert(errorText(e)))}>
        {t("update.openPage")}
      </button>
    </div>
  );
}
