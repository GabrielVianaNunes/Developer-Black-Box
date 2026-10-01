import { useI18n } from "../../i18n";
import type { ExclusionKind, Settings } from "../../types/dashboard";
import { AppPicker, useAppCandidates } from "./AppPicker";
import { canToggle, KINDS, newRule, rulesFrom, toggle, toSettings, type Rule } from "./exclusionRules";

type Props = {
  settings: Pick<Settings, "excludedApps" | "partialExclusions">;
  onChange: (next: Pick<Settings, "excludedApps" | "partialExclusions">) => void;
};

const KIND_LABEL: Record<ExclusionKind, "privacy.rule.lifecycle" | "privacy.rule.metrics" | "privacy.rule.crashes"> = {
  lifecycle: "privacy.rule.lifecycle",
  metrics: "privacy.rule.metrics",
  crashes: "privacy.rule.crashes",
};

/**
 * Regras de exclusão: para cada programa, o que CONTINUA sendo gravado. Por padrão nada (o programa inteiro fica
 * de fora); marcar um tipo é uma escolha consciente de gravar mais.
 */
export function ExclusionRulesCard({ settings, onChange }: Props) {
  const { t } = useI18n();
  const rules = rulesFrom(settings);
  const { list } = useAppCandidates(rules.length > 0);
  const friendly = new Map((list ?? []).map((c) => [c.exe, c.name]));

  const replace = (next: Rule[]) => onChange(toSettings(next));

  return (
    <div className="card">
      <h2>{t("privacy.excludedTitle")}</h2>
      <p className="muted small">{t("privacy.excludedHelp")}</p>

      {rules.length === 0 && <p className="muted small">{t("privacy.none")}</p>}
      <ul className="plain rules">
        {rules.map((rule) => {
          const name = friendly.get(rule.exe);
          const label = name && name.toLowerCase() !== rule.exe.replace(/[.]exe$/, "") ? name : rule.exe;
          const recordedAny = KINDS.some((k) => rule.recorded[k]);
          return (
            <li key={rule.exe} className="rule">
              <div className="rule-head">
                <span className="rule-name">{label}</span>
                {label !== rule.exe && <span className="muted small">{rule.exe}</span>}
                <button
                  className="rule-remove"
                  aria-label={t("privacy.remove", { name: label })}
                  onClick={() => replace(rules.filter((r) => r.exe !== rule.exe))}
                >
                  ×
                </button>
              </div>
              <fieldset className="rule-kinds">
                <legend className="muted small">{t("privacy.rule.records")}</legend>
                {KINDS.map((kind) => (
                  <label key={kind} className="check">
                    <input
                      type="checkbox"
                      checked={rule.recorded[kind]}
                      disabled={!canToggle(rule, kind)}
                      onChange={() => replace(rules.map((r) => (r.exe === rule.exe ? toggle(r, kind) : r)))}
                    />
                    {t(KIND_LABEL[kind])}
                  </label>
                ))}
              </fieldset>
              <p className="muted small">{recordedAny ? t("privacy.rule.partial") : t("privacy.rule.nothing")}</p>
            </li>
          );
        })}
      </ul>
      {rules.length > 0 && <p className="muted small">{t("privacy.rule.hints")}</p>}

      <AppPicker taken={rules.map((r) => r.exe)} onAdd={(exe) => replace([...rules, newRule(exe)])} />
    </div>
  );
}
