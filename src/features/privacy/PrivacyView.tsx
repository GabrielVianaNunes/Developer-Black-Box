import { usePolling } from "../../app/hooks";
import { useI18n } from "../../i18n";
import type { PrivacyDraft } from "../../app/usePrivacyDraft";
import { getConfigHistory } from "../../services/backend";
import type { Status } from "../../types/status";
import { AppPicker, useAppCandidates } from "./AppPicker";
import { AuthorizationsCard } from "./AuthorizationsCard";
import { ExclusionRulesCard } from "./ExclusionRulesCard";
import { StartupCard } from "./StartupCard";
import { NewsCard } from "./NewsCard";
import { UpdatesCard } from "./UpdatesCard";

export function PrivacyView({ status, onShowNews, privacy }: { status: Status; onShowNews: () => void; privacy: PrivacyDraft }) {
  const { t, f, label } = useI18n();
  const { saved, draft, setDraft, msg, dirty, apply, discard } = privacy;
  const history = usePolling(getConfigHistory, 5000);

  if (!draft || !saved) return <p className="muted">{msg ?? t("app.loading")}</p>;

  return (
    <section aria-label={t("nav.privacy")}>
      <div className="card">
        <h2>{t("privacy.guardTitle")}</h2>
        <p>{t("privacy.currentState", { text: status.text })}</p>
        <p className="muted">{t("privacy.guardText")}</p>
      </div>

      <AppList
        title={t("privacy.protectedTitle")}
        help={t("privacy.protectedHelp")}
        items={draft.protectedApps}
        onChange={(v) => setDraft({ ...draft, protectedApps: v })}
      />
      <ExclusionRulesCard settings={draft} onChange={(next) => setDraft({ ...draft, ...next })} />

      <div className="card">
        <h2>{t("privacy.recordingTitle")}</h2>
        <label className="inline">
          {t("privacy.stability")}
          <input
            type="number"
            min={1}
            max={60}
            value={draft.stabilityWindowMs / 1000}
            onChange={(e) => setDraft({ ...draft, stabilityWindowMs: Math.round(Number(e.target.value) * 1000) })}
          />
        </label>
        <p className="muted small">{t("privacy.stabilityHelp")}</p>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.autoStart}
            onChange={(e) => setDraft({ ...draft, autoStart: e.target.checked })}
          />
          {t("privacy.autoStart")}
        </label>
        <p className="muted small">{t("privacy.autoStartHelp")}</p>
        <div className="row">
          <button className="primary" disabled={!dirty} onClick={() => void apply()}>{t("privacy.apply")}</button>
          <button disabled={!dirty} onClick={discard}>{t("privacy.discard")}</button>
        </div>
        {msg && <p className="notice" role="status">{msg}</p>}
      </div>

      <StartupCard />
      <UpdatesCard />
      <NewsCard onShow={onShowNews} />

      <AuthorizationsCard protectedApps={saved.protectedApps} />

      <div className="card">
        <h2>{t("privacy.historyTitle")}</h2>
        <p className="muted small">{t("privacy.historyHelp")}</p>
        <ul className="plain">
          {(history.data ?? []).map((h, i) => (
            <li key={i}>
              <span className="muted small">{f.time(h.atUtcMs)}</span>{" "}
              {t("privacy.historyEntry", {
                key: label("configKey", h.key),
                change: label("configChange", h.change),
              })}
            </li>
          ))}
        </ul>
        {history.data && history.data.length === 0 && <p className="muted">{t("privacy.historyEmpty")}</p>}
      </div>
    </section>
  );
}

function AppList({
  title,
  help,
  items,
  onChange,
}: {
  title: string;
  help: string;
  items: string[];
  onChange: (v: string[]) => void;
}) {
  const { t } = useI18n();
  // Nome amigável dos itens já escolhidos (quando o programa é conhecido neste PC).
  const { list } = useAppCandidates(items.length > 0);
  const friendly = new Map((list ?? []).map((c) => [c.exe, c.name]));
  return (
    <div className="card">
      <h2>{title}</h2>
      <p className="muted small">{help}</p>
      <div className="chips">
        {items.map((a) => {
          const name = friendly.get(a);
          return (
            <span key={a} className="chip" title={name ? a : undefined}>
              {name && name.toLowerCase() !== (a.endsWith(".exe") ? a.slice(0, -4) : a) ? name : a}
              <button aria-label={t("privacy.remove", { name: name ?? a })} onClick={() => onChange(items.filter((x) => x !== a))}>
                ×
              </button>
            </span>
          );
        })}
        {items.length === 0 && <span className="muted small">{t("privacy.none")}</span>}
      </div>
      <AppPicker taken={items} onAdd={(exe) => onChange([...items, exe].sort())} />
    </div>
  );
}
