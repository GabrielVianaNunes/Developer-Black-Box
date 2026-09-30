import { LANGS, useI18n } from "../i18n";

/** Troca de idioma a qualquer momento, na própria interface. Os nomes ficam na língua de cada um. */
export function LanguageSwitch() {
  const { lang, setLang, t } = useI18n();
  return (
    <div className="langswitch" role="group" aria-label={t("lang.label")}>
      {LANGS.map((l) => (
        <button
          key={l}
          type="button"
          lang={l}
          className={l === lang ? "lang active" : "lang"}
          aria-pressed={l === lang}
          onClick={() => void setLang(l)}
        >
          {l === "en" ? t("lang.en") : t("lang.ptBR")}
        </button>
      ))}
    </div>
  );
}
