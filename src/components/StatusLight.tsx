import { useI18n } from "../i18n";
import type { Light } from "../types/status";

/** A cor nunca é o único sinal: há rótulo acessível e o texto do estado ao lado. */
export function StatusLight({ light }: { light: Light }) {
  const { t } = useI18n();
  const label = t(light === "green" ? "light.green" : light === "red" ? "light.red" : "light.gray");
  return <span className={`light light-${light}`} role="img" aria-label={label} title={label} />;
}
