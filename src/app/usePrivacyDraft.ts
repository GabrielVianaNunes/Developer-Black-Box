import { useCallback, useEffect, useState } from "react";
import { isDirty } from "../features/privacy/draft";
import { useI18n } from "../i18n";
import { getSettings, setSettings } from "../services/backend";
import type { Settings } from "../types/dashboard";

/**
 * O rascunho das configurações de privacidade (listas de protegidos e exclusões, etc.).
 *
 * Vive no topo do app, e não dentro da aba Privacidade: trocar de aba NUNCA descarta alterações não aplicadas, e a barra
 * de "alterações não salvas" aparece em qualquer aba. Uma regra só vale depois de Aplicar.
 */
export function usePrivacyDraft() {
  const { t, errorText } = useI18n();
  const [saved, setSaved] = useState<Settings | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    getSettings()
      .then((s) => {
        if (!alive) return;
        setSaved(s);
        setDraft(s);
      })
      .catch((e) => alive && setMsg(errorText(e)));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const apply = useCallback(async () => {
    if (!draft) return;
    setMsg(null);
    try {
      const s = await setSettings(draft);
      setSaved(s);
      setDraft(s);
      setMsg(t("privacy.applied"));
    } catch (e) {
      setMsg(errorText(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft]);

  const discard = useCallback(() => {
    setDraft(saved);
    setMsg(null);
  }, [saved]);

  return { saved, draft, setDraft, msg, setMsg, dirty: isDirty(saved, draft), apply, discard };
}

export type PrivacyDraft = ReturnType<typeof usePrivacyDraft>;
