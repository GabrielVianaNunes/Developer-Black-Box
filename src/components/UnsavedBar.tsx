import type { PrivacyDraft } from "../app/usePrivacyDraft";
import { useI18n } from "../i18n";

interface Props {
  draft: PrivacyDraft;
  /** Está na aba Privacidade? Fora dela a barra oferece ir até lá. */
  onPrivacyTab: boolean;
  goToPrivacy: () => void;
}

/**
 * Aviso fixo de "alterações de privacidade ainda não aplicadas", visível em QUALQUER aba. Uma regra só vale depois de
 * Aplicar; sem este aviso a pessoa podia achar que um programa estava protegido ou excluído sem estar.
 */
export function UnsavedBar({ draft, onPrivacyTab, goToPrivacy }: Props) {
  const { t } = useI18n();
  if (!draft.dirty) return null;
  return (
    <div className="unsaved-bar" role="alert" data-testid="unsaved-bar">
      <div className="unsaved-text">
        <strong>{t("privacy.unsaved.title")}</strong>
        <span className="muted small"> {t("privacy.unsaved.text")}</span>
        {draft.msg && <span className="small"> {draft.msg}</span>}
      </div>
      <div className="unsaved-actions">
        <button className="primary" onClick={() => void draft.apply()}>{t("privacy.apply")}</button>
        <button onClick={draft.discard}>{t("privacy.discard")}</button>
        {!onPrivacyTab && <button onClick={goToPrivacy}>{t("privacy.unsaved.review")}</button>}
      </div>
    </div>
  );
}
