import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { getGuideState, setNewsEnabled } from "../../services/backend";

/** Novidades depois de uma atualização: dá para desligar, e dá para ver de novo quando quiser. */
export function NewsCard({ onShow }: { onShow: () => void }) {
  const { t, errorText } = useI18n();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    getGuideState()
      .then((g) => setEnabled(g.newsEnabled))
      .catch((e) => setMsg(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function toggle(next: boolean) {
    setMsg(null);
    try {
      setEnabled((await setNewsEnabled(next)).newsEnabled);
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  return (
    <div className="card">
      <h2>{t("news.title")}</h2>
      <label className="check">
        <input type="checkbox" disabled={enabled === null} checked={!!enabled} onChange={(e) => toggle(e.target.checked)} />
        {t("news.checkbox")}
      </label>
      <p className="muted small">{t("news.help")}</p>
      <div className="actions">
        <button onClick={onShow}>{t("news.show")}</button>
      </div>
      {msg && <p className="notice" role="status">{msg}</p>}
    </div>
  );
}
