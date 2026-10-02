import type { Detection, Settings } from "../../types/dashboard";
import { newRule, rulesFrom, toSettings } from "./exclusionRules.ts";

/**
 * Programas de sistema compartilhados por muitos apps: proteger um deles suspenderia a gravação sempre que QUALQUER app
 * que o usa estiver em primeiro plano. Servem só para avisar a pessoa; a decisão continua dela.
 */
export const SHARED_HOSTS = [
  "msedgewebview2.exe",
  "applicationframehost.exe",
  "explorer.exe",
  "svchost.exe",
  "dllhost.exe",
  "runtimebroker.exe",
];

export function isSharedHost(exe: string): boolean {
  return SHARED_HOSTS.includes(exe.toLowerCase());
}

type Lists = Pick<Settings, "protectedApps" | "excludedApps" | "partialExclusions">;

export function isProtected(s: Pick<Settings, "protectedApps">, exe: string): boolean {
  return s.protectedApps.includes(exe);
}

/** O programa já tem regra de exclusão (total ou só de alguns tipos de evento)? */
export function hasExclusionRule(s: Pick<Settings, "excludedApps" | "partialExclusions">, exe: string): boolean {
  return s.excludedApps.includes(exe) || s.partialExclusions.some((p) => p.exe === exe);
}

/** A lista de protegidos com o programa, sem repetir e em ordem. */
export function addProtected(s: Pick<Settings, "protectedApps">, exe: string): Pick<Settings, "protectedApps"> {
  return { protectedApps: [...new Set([...s.protectedApps, exe])].sort() };
}

/** Uma exclusão por INTEIRO (o padrão seguro) para o programa; qualquer regra parcial dele é substituída. */
export function addExclusion(s: Lists, exe: string): Pick<Settings, "excludedApps" | "partialExclusions"> {
  return toSettings([...rulesFrom(s).filter((r) => r.exe !== exe), newRule(exe)]);
}

export type Outcome = { kind: "found"; exe: string } | { kind: "self" } | { kind: "unknown" };

/** O que mostrar para o resultado da detecção. O próprio app nunca vira "encontrado". */
export function outcome(d: Detection): Outcome {
  if (d.isSelf) return { kind: "self" };
  return d.exe ? { kind: "found", exe: d.exe } : { kind: "unknown" };
}
