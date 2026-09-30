import type { Light } from "../types/status";

const LABEL: Record<Light, string> = {
  green: "Luz verde: gravação ativa",
  red: "Luz vermelha: gravação inativa",
  gray: "Luz cinza: iniciando ou encerrando",
};

/** A cor nunca é o único sinal: há rótulo acessível e o texto do estado ao lado. */
export function StatusLight({ light }: { light: Light }) {
  return <span className={`light light-${light}`} role="img" aria-label={LABEL[light]} title={LABEL[light]} />;
}
