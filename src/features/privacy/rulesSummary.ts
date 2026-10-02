import type { OmittedCount, Settings } from "../../types/dashboard";
import { isExcludedWhole, rulesFrom } from "./exclusionRules.ts";

/**
 * Resumo das regras que estão valendo, para a Visão Geral. Só nomes de programas que a própria pessoa escolheu e um
 * número por regra (quantas vezes ela deixou algo de fora): nunca conteúdo, títulos nem caminhos.
 */
export interface RuleRow {
  exe: string;
  /** "whole": o programa inteiro fica de fora; "partial": só alguns tipos de evento. */
  kind: "whole" | "partial";
  /** Vale também para o que o programa inicia. */
  children: boolean;
  /** Vezes que a regra deixou algo de fora desde que o app abriu (0 se ainda nenhuma). */
  omitted: number;
}

export interface RulesSummary {
  protectedCount: number;
  rows: RuleRow[];
  /** Soma dos omitidos de todas as regras. */
  omittedTotal: number;
}

export function summarize(settings: Pick<Settings, "protectedApps" | "excludedApps" | "partialExclusions" | "excludedTrees">, omitted: OmittedCount[]): RulesSummary {
  const counts = new Map(omitted.map((o) => [o.exe, o.count]));
  const rows: RuleRow[] = rulesFrom(settings).map((r) => {
    return { exe: r.exe, kind: isExcludedWhole(r.recorded) ? "whole" : "partial", children: r.children, omitted: counts.get(r.exe) ?? 0 };
  });
  return {
    protectedCount: settings.protectedApps.length,
    rows,
    omittedTotal: rows.reduce((n, r) => n + r.omitted, 0),
  };
}

/**
 * Programas que não são "um app" para o usuário, e sim um hospedeiro de várias coisas: proteger um deles muda muito
 * mais do que parece. Só o nome do executável, em minúsculas.
 */
const HOSTS = new Set(["msedgewebview2.exe", "applicationframehost.exe", "explorer.exe", "svchost.exe", "dllhost.exe", "runtimebroker.exe"]);

export function isKnownHost(exe: string): boolean {
  return HOSTS.has(exe.toLowerCase());
}
