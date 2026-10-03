/** Formata os valores NUMÉRICOS do inventário (o backend nunca guarda texto). Puro: recebe a função de tradução. */

type T = (key: string, params?: Record<string, string | number>) => string;

/** Valores abaixo disto cabem sem perda em um número do JavaScript; acima, o código é um hash. */
const HASHED_FLAG = 2 ** 52;
const COMPONENT_BITS = 13;

/** Versão pontuada empacotada (4 componentes de 13 bits) ou, se for hash, o texto "formato não numérico". */
export function versionText(v: number, t: T): string {
  if (v >= HASHED_FLAG) return t("inventory.value.hashed");
  const parts: number[] = [];
  let rest = v;
  for (let i = 0; i < 4; i++) {
    parts.unshift(rest % 2 ** COMPONENT_BITS);
    rest = Math.floor(rest / 2 ** COMPONENT_BITS);
  }
  while (parts.length > 1 && parts[parts.length - 1] === 0) parts.pop();
  return parts.join(".");
}

export function inventoryValueText(item: string, v: number | null, t: T): string {
  if (v == null) return t("inventory.value.unavailable");
  switch (item) {
    case "BiosVersion":
      return versionText(v, t);
    case "BiosDate": {
      const s = String(v).padStart(8, "0");
      return `${s.slice(0, 4)}-${s.slice(4, 6)}-${s.slice(6, 8)}`;
    }
    case "FirmwareType":
      return v === 2 ? t("inventory.value.uefi") : v === 1 ? t("inventory.value.legacy") : String(v);
    case "SecureBoot":
      return v ? t("inventory.value.on") : t("inventory.value.off");
    case "OsBuild":
      return `${Math.floor(v / 2 ** 20)}.${v % 2 ** 20}`;
    case "DeviceProblemCodes": {
      // máscara: bit N = código N (o bit 0 reúne os códigos acima de 52)
      const codes: string[] = [];
      let rest = v;
      for (let bit = 0; rest > 0 && bit < 53; bit++) {
        if (rest % 2 === 1) codes.push(bit === 0 ? "53+" : String(bit));
        rest = Math.floor(rest / 2);
      }
      return codes.length ? codes.join(", ") : t("inventory.value.none");
    }
    default:
      return String(v);
  }
}
