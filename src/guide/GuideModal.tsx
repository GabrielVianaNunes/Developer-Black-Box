import { useEffect, useId, useRef, useState } from "react";
import { LanguageSwitch } from "../components/LanguageSwitch";
import { useI18n } from "../i18n";
import { DEMOS } from "./demos";
import type { Key } from "../i18n";
import type { Step } from "./steps";

interface Props {
  steps: Step[];
  /** O usuário terminou (`finished`) ou pulou o guia. */
  onClose: (finished: boolean) => void;
  /** Título pequeno acima dos passos (as novidades dizem de qual versão são). */
  heading?: string;
  /** Textos dos botões de pular e de terminar (o tour usa os padrões). */
  labels?: { skip: Key; done: Key };
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select, [tabindex]:not([tabindex="-1"])';

/**
 * O guia: uma janela por cima do app, passo a passo, que SEMPRE pode ser pulada (botão "Pular" e tecla Esc).
 * O foco fica preso na janela enquanto ela está aberta e volta ao lugar de onde veio ao fechar.
 */
export function GuideModal({ steps, onClose, heading, labels = { skip: "guide.skip", done: "guide.done" } }: Props) {
  const { t } = useI18n();
  const [index, setIndex] = useState(0);
  const titleId = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);
  const last = index === steps.length - 1;
  const step = steps[index];
  const Demo = DEMOS[step.id];

  useEffect(() => {
    opener.current = document.activeElement;
    dialog.current?.querySelector<HTMLElement>("[data-initial]")?.focus();
    return () => {
      if (opener.current instanceof HTMLElement) opener.current.focus();
    };
  }, []);

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose(false);
      return;
    }
    if (e.key !== "Tab" || !dialog.current) return;
    const items = [...dialog.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    if (items.length === 0) return;
    const first = items[0];
    const end = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      end.focus();
    } else if (!e.shiftKey && document.activeElement === end) {
      e.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="guide-overlay" role="presentation" onKeyDown={onKeyDown}>
      <div className="guide card" role="dialog" aria-modal="true" aria-labelledby={titleId} ref={dialog}>
        <div className="guide-top">
          <span className="muted small">{t("guide.step", { n: index + 1, total: steps.length })}</span>
          <LanguageSwitch />
        </div>
        {heading && <p className="guide-heading">{heading}</p>}
        <h2 id={titleId}>{t(step.title)}</h2>
        {step.body.map((k) => (
          <p key={k}>{t(k)}</p>
        ))}
        <Demo />
        <div className="guide-dots" aria-hidden="true">
          {steps.map((s, i) => (
            <span key={s.id} className={i === index ? "dot on" : "dot"} />
          ))}
        </div>
        <div className="guide-actions">
          <button onClick={() => onClose(false)}>{t(labels.skip)}</button>
          <span className="demo-grow" />
          <button disabled={index === 0} onClick={() => setIndex(index - 1)}>{t("guide.back")}</button>
          {last ? (
            <button className="primary" data-initial onClick={() => onClose(true)}>{t(labels.done)}</button>
          ) : (
            <button className="primary" data-initial onClick={() => setIndex(index + 1)}>{t("guide.next")}</button>
          )}
        </div>
      </div>
    </div>
  );
}
