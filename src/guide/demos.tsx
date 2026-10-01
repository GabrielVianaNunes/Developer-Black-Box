import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import type { DemoId } from "./steps";

/**
 * Moldura de todo exemplo do guia. O conteúdo é INERTE: `inert` impede foco e clique, é marcado como ilustração
 * e usa só dados inventados. Nenhum exemplo importa ou chama o backend: o guia mostra o que o app faz sem fazer.
 */
function DemoFrame({ label, children }: { label: string; children: ReactNode }) {
  const { t } = useI18n();
  return (
    <figure className="demo" aria-label={label}>
      <figcaption className="demo-tag">{t("guide.example")}</figcaption>
      <div className="demo-body" inert aria-hidden="true">
        {children}
      </div>
      <p className="muted small demo-note">{t("guide.exampleNote")}</p>
    </figure>
  );
}

function Light({ color }: { color: "green" | "red" }) {
  return <span className={`light light-${color}`} aria-hidden="true" />;
}

function Welcome() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.demo.box")}>
      <div className="demo-flow">
        <span className="demo-box">{t("guide.demo.pc")}</span>
        <span className="demo-arrow">→</span>
        <span className="demo-box demo-strong">{t("guide.demo.box")}</span>
        <span className="demo-arrow">→</span>
        <span className="demo-box">{t("guide.demo.stays")}</span>
      </div>
    </DemoFrame>
  );
}

function LightStates() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.tour.light.title")}>
      <ul className="plain demo-states">
        <li><Light color="green" /> {t("guide.demo.recording")}</li>
        <li><Light color="red" /> {t("guide.demo.pausedManually")}</li>
        <li><Light color="red" /> {t("guide.demo.protectedFront")}</li>
      </ul>
    </DemoFrame>
  );
}

function PausedBar() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.tour.paused.title")}>
      <div className="demo-bar">
        <Light color="red" />
        <span className="demo-grow">{t("guide.demo.pausedManually")}</span>
        <button className="primary" disabled>{t("app.pause")}</button>
        <button className="primary demo-focus" disabled>{t("app.resume")}</button>
      </div>
    </DemoFrame>
  );
}

function PrivacyList() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.tour.privacy.title")}>
      <div className="chips">
        {["browser-example.exe", "password-manager-example.exe"].map((n) => (
          <span key={n} className="chip">{n}</span>
        ))}
      </div>
      <p className="small">{t("guide.demo.suspended")}</p>
    </DemoFrame>
  );
}

function IncidentRow() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.tour.incidents.title")}>
      <div className="demo-row">
        <span className="badge">!</span>
        <span className="demo-grow">
          <strong>{t("guide.demo.incident")}</strong>
          <span className="muted small"> · synthetic-app.exe</span>
        </span>
        <button disabled>{t("guide.demo.export")}</button>
      </div>
    </DemoFrame>
  );
}

function RuleRow() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.tour.rules.title")}>
      <div className="rule">
        <div className="rule-head"><span className="rule-name">synthetic-app.exe</span></div>
        <fieldset className="rule-kinds">
          <legend className="muted small">{t("privacy.rule.records")}</legend>
          {(["privacy.rule.lifecycle", "privacy.rule.metrics", "privacy.rule.crashes"] as const).map((k) => (
            <label key={k} className="check">
              <input type="checkbox" checked={false} readOnly disabled /> {t(k)}
            </label>
          ))}
        </fieldset>
        <p className="muted small">{t("privacy.rule.nothing")}</p>
      </div>
    </DemoFrame>
  );
}

function Extras() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.tour.extras.title")}>
      <div className="demo-row">
        <div className="langswitch" role="group" aria-label={t("lang.label")}>
          <span className="lang active">{t("lang.en")}</span>
          <span className="lang">{t("lang.ptBR")}</span>
        </div>
      </div>
      <label className="check">
        <input type="checkbox" checked={false} readOnly disabled /> {t("update.checkbox")}
      </label>
    </DemoFrame>
  );
}

function Done() {
  const { t } = useI18n();
  return (
    <DemoFrame label={t("guide.open")}>
      <div className="demo-row">
        <span className="guide-button demo-focus" aria-hidden="true">?</span>
        <span className="demo-grow">{t("guide.open")}</span>
      </div>
    </DemoFrame>
  );
}

/** Um exemplo para cada passo (o compilador exige todos). */
export const DEMOS: Record<DemoId, () => ReactNode> = {
  welcome: Welcome,
  light: LightStates,
  paused: PausedBar,
  privacy: PrivacyList,
  incidents: IncidentRow,
  rules: RuleRow,
  extras: Extras,
  done: Done,
};
