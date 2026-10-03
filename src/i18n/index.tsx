import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { getLanguage, setLanguage as saveLanguage } from "../services/backend";
import type { Detail } from "../types/dashboard";
import { en } from "./en";
import { inventoryValueText } from "./inventory";
import { ptBR } from "./pt-BR";

export type Lang = "en" | "pt-BR";
export type Key = keyof typeof en;
export type Params = Record<string, string | number>;

export const LANGS: Lang[] = ["en", "pt-BR"];
const DICTS: Record<Lang, Record<Key, string>> = { en, "pt-BR": ptBR };
const LOCALE: Record<Lang, string> = { en: "en-US", "pt-BR": "pt-BR" };

export const isLang = (v: unknown): v is Lang => v === "en" || v === "pt-BR";

/** Traduz `key` para `lang`, trocando os marcadores {nome} pelos parâmetros. */
export function translate(lang: Lang, key: Key, params?: Params): string {
  const text = DICTS[lang][key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole));
}

/** Idioma inicial provisório (antes de o backend responder): o do navegador/sistema. */
export function initialLang(): Lang {
  try {
    return navigator.language?.toLowerCase().startsWith("pt") ? "pt-BR" : "en";
  } catch {
    return "en";
  }
}

/** Formatadores que respeitam o idioma escolhido (separador decimal, data, hora). */
export interface Formatters {
  time(ms: number): string;
  clock(ms: number): string;
  bytes(n: number): string;
  kb(kb: number): string;
  pct(permille: number): string;
  offset(ms: number): string;
  remaining(ms: number): string;
}

export function makeFormatters(lang: Lang): Formatters {
  const locale = LOCALE[lang];
  const one = new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const bytes = (n: number): string => {
    if (n < 1024) return `${n} B`;
    const units = ["KB", "MB", "GB"];
    let v = n / 1024;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${one.format(v)} ${units[i]}`;
  };
  return {
    time: (ms) => new Date(ms).toLocaleString(locale),
    clock: (ms) => new Date(ms).toLocaleTimeString(locale),
    bytes,
    kb: (kb) => bytes(kb * 1024),
    pct: (permille) => `${one.format(permille / 10)}%`,
    offset: (ms) => {
      const sign = ms < 0 ? "-" : "+";
      const abs = Math.abs(ms);
      const s = Math.floor(abs / 1000);
      const mm = String(Math.floor(s / 60)).padStart(2, "0");
      const ss = String(s % 60).padStart(2, "0");
      const dec = lang === "pt-BR" ? "," : ".";
      return `${sign}${mm}:${ss}${dec}${Math.floor((abs % 1000) / 100)}`;
    },
    remaining: (ms) => {
      const s = Math.max(0, Math.ceil(ms / 1000));
      const h = Math.floor(s / 3600);
      const m = Math.floor((s % 3600) / 60);
      return h > 0 ? `${h} h ${String(m).padStart(2, "0")} min` : `${m} min ${String(s % 60).padStart(2, "0")} s`;
    },
  };
}

export interface I18n {
  lang: Lang;
  setLang: (lang: Lang) => Promise<void>;
  t: (key: Key, params?: Params) => string;
  f: Formatters;
  /** Texto de `prefixo.código` (ex. "configKey", "protected_apps"); o próprio código se não existir. */
  label: (prefix: string, code: string) => string;
  /** Rótulo de um tipo de evento (ex. "ProcessStarted"); o próprio código se for desconhecido. */
  kindLabel: (kind: string) => string;
  incidentKindLabel: (kind: string) => string;
  severityLabel: (v: string) => string;
  investigationLabel: (v: string) => string;
  /** Detalhe de um evento, formatado a partir do código e dos números. */
  detailText: (d: Detail) => string;
  /** Resumo de um incidente (código com parâmetros, ex. "cpu_sustained|900|3"). */
  summaryText: (summary: string) => string;
  /** Mensagem para um código de erro do backend (ou uma mensagem genérica). */
  errorText: (e: unknown) => string;
}

const Ctx = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(initialLang);

  // O backend é a fonte da verdade do idioma escolhido (salvo, ou o do Windows na 1ª execução).
  useEffect(() => {
    let alive = true;
    getLanguage()
      .then((code) => alive && isLang(code) && setLangState(code))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    document.documentElement.lang = lang;
    document.title = translate(lang, "app.title");
  }, [lang]);

  const setLang = useCallback(async (next: Lang) => {
    setLangState(next); // troca na hora, sem recarregar
    try {
      const saved = await saveLanguage(next); // salva e retraduz a bandeja
      if (isLang(saved)) setLangState(saved);
    } catch {
      /* a interface fica no idioma escolhido nesta sessão mesmo se salvar falhar */
    }
  }, []);

  const value = useMemo<I18n>(() => {
    const t = (key: Key, params?: Params) => translate(lang, key, params);
    const f = makeFormatters(lang);
    const has = (key: string): key is Key => key in DICTS[lang];
    const lookup = (prefix: string, code: string) => (has(`${prefix}.${code}`) ? t(`${prefix}.${code}` as Key) : code);

    // As chaves do inventário são montadas em tempo de execução (um texto por valor), por isso o tradutor é solto.
    const tAny = (key: string, params?: Record<string, string | number>) => t(key as Key, params);

    const detailText = (d: Detail): string => {
      switch (d.code) {
        case "processStarted":
          return t("detail.processStarted", { ppid: d.parentPid });
        case "processExited":
          return d.exitCode == null
            ? t("detail.processExitedUnknown")
            : t("detail.processExitedCode", { code: d.exitCode });
        case "processMetrics":
          return t("detail.processMetrics", { cpu: f.pct(d.cpuPermille), mem: f.kb(d.workingSetKb) });
        case "systemMetrics":
          return t("detail.systemMetrics", {
            cpu: f.pct(d.cpuPermille),
            used: f.kb(d.memUsedKb),
            total: f.kb(d.memTotalKb),
          });
        case "appCrash":
          return t("detail.appCrash", { code: `0x${d.exceptionCode.toString(16).toUpperCase().padStart(8, "0")}` });
        case "appHang":
          return t("detail.appHang");
        case "healthEvent": {
          const category = lookup("health.category", d.category);
          if (d.value == null) return t("detail.healthEvent", { category, id: d.eventId });
          // Contagem de quedas do serviço em decimal; os demais números são códigos de erro, em hexadecimal.
          return d.category === "ServiceCrash"
            ? t("detail.healthEventCount", { category, id: d.eventId, n: d.value })
            : t("detail.healthEventCode", {
                category,
                id: d.eventId,
                code: `0x${d.value.toString(16).toUpperCase().padStart(8, "0")}`,
              });
        }
        case "powerStatus":
          return t("detail.powerStatus", {
            ac: d.ac === "online" ? t("power.ac.online") : d.ac === "offline" ? t("power.ac.offline") : t("power.ac.unknown"),
            charge: d.chargePercent == null ? t("power.charge.unknown") : `${d.chargePercent}%`,
          });
        case "inventoryChange":
          return t("detail.inventoryChange", {
            item: lookup("inventory.item", d.item),
            from: inventoryValueText(d.item, d.previous, tAny),
            to: inventoryValueText(d.item, d.current, tAny),
          });
        case "userMarker":
          return t("detail.userMarker", { n: d.marker });
        case "recorderStateChanged":
          return t("detail.recorderStateChanged");
        default:
          return t("detail.unknown");
      }
    };

    const summaryText = (summary: string): string => {
      const [code, ...args] = summary.split("|");
      const num = (i: number) => Number(args[i]);
      switch (code) {
        case "manual":
          return t("summary.manual");
        case "cpu_sustained":
          return t("summary.cpu_sustained", { pct: f.pct(num(0)), n: num(1) });
        case "memory_high":
          return t("summary.memory_high", { mb: num(0) });
        case "app_crash":
          return t("summary.app_crash", { code: `0x${num(0).toString(16).toUpperCase().padStart(8, "0")}` });
        case "app_hang":
          return t("summary.app_hang");
        default:
          return summary; // incidente antigo, já gravado como texto: mostra como está
      }
    };

    const errorText = (e: unknown): string => {
      if (typeof e === "string" && has(`error.${e}`)) return t(`error.${e}` as Key);
      return t("error.generic");
    };

    return {
      lang,
      setLang,
      t,
      f,
      label: lookup,
      kindLabel: (k) => lookup("kind", k),
      incidentKindLabel: (k) => lookup("incidentKind", k),
      severityLabel: (v) => lookup("severity", v),
      investigationLabel: (v) => lookup("investigation", v),
      detailText,
      summaryText,
      errorText,
    };
  }, [lang, setLang]);

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useI18n(): I18n {
  const v = useContext(Ctx);
  if (!v) throw new Error("useI18n must be used inside <I18nProvider>");
  return v;
}
