import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { detectForegroundApp } from "../../services/backend";
import type { Settings } from "../../types/dashboard";
import { addExclusion, addProtected, hasExclusionRule, isProtected, isSharedHost, outcome, type Outcome } from "./detect";

const SECONDS = 5;

interface Props {
  settings: Settings;
  /** Atualiza o RASCUNHO (nada vale até Aplicar). */
  onChange: (next: Partial<Settings>) => void;
}

type Phase = { name: "idle" } | { name: "counting"; left: number } | { name: "done"; result: Outcome };

/**
 * "Detectar o app em primeiro plano": a pessoa clica, traz o outro app para a frente durante a contagem e o app mostra o
 * NOME do executável que o Guard enxerga, com atalhos para adicioná-lo às listas. Só o nome é lido (nunca o título) e
 * nada é gravado ou enviado: o resultado vive só nesta tela.
 */
export function DetectForegroundCard({ settings, onChange }: Props) {
  const { t, errorText } = useI18n();
  const [phase, setPhase] = useState<Phase>({ name: "idle" });
  const [added, setAdded] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearInterval(timer.current), []);

  async function detect() {
    setAdded(null);
    setPhase({ name: "counting", left: SECONDS });
    window.clearInterval(timer.current);
    timer.current = window.setInterval(() => {
      setPhase((p) => (p.name === "counting" ? { name: "counting", left: Math.max(1, p.left - 1) } : p));
    }, 1000);
    try {
      const result = await detectForegroundApp(SECONDS * 1000);
      setPhase({ name: "done", result: outcome(result) });
    } catch (e) {
      setPhase({ name: "idle" });
      setAdded(errorText(e));
    } finally {
      window.clearInterval(timer.current);
    }
  }

  const result = phase.name === "done" ? phase.result : null;
  return (
    <div className="card">
      <h2>{t("privacy.detect.title")}</h2>
      <p className="muted small">{t("privacy.detect.help")}</p>
      <div className="row">
        <button disabled={phase.name === "counting"} onClick={() => void detect()}>
          {phase.name === "counting" ? t("privacy.detect.counting", { n: phase.left }) : t("privacy.detect.button", { n: SECONDS })}
        </button>
      </div>
      <div aria-live="polite">
        {result?.kind === "unknown" && <p className="notice">{t("privacy.detect.unknown")}</p>}
        {result?.kind === "self" && <p className="notice">{t("privacy.detect.self")}</p>}
        {result?.kind === "found" && <Found exe={result.exe} />}
        {added && <p className="notice" role="status">{added}</p>}
      </div>
    </div>
  );

  function Found({ exe }: { exe: string }) {
    const inProtected = isProtected(settings, exe);
    const inExclusions = hasExclusionRule(settings, exe);
    return (
      <div className="detected">
        <p>
          <strong>{t("privacy.detect.found", { exe })}</strong>
        </p>
        {isSharedHost(exe) && <p className="notice">{t("privacy.detect.host")}</p>}
        <div className="row">
          <button
            disabled={inProtected}
            onClick={() => {
              onChange(addProtected(settings, exe));
              setAdded(t("privacy.detect.added"));
            }}
          >
            {inProtected ? t("privacy.detect.alreadyProtected") : t("privacy.detect.addProtected")}
          </button>
          <button
            disabled={inExclusions}
            onClick={() => {
              onChange(addExclusion(settings, exe));
              setAdded(t("privacy.detect.added"));
            }}
          >
            {inExclusions ? t("privacy.detect.alreadyExcluded") : t("privacy.detect.addExcluded")}
          </button>
        </div>
      </div>
    );
  }
}
