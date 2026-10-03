/** Modelo puro da aba "Saúde do sistema": só organiza o que o backend já devolveu (nada novo é lido aqui). */
import type { ActivityRow, HealthSource } from "../../types/dashboard";

/** As fontes e os estados são enumerações fechadas do backend. */
export const SOURCES = ["eventLog", "inventory", "power", "telemetry"] as const;
export const STATES = ["ok", "attention", "unavailable", "waiting", "paused", "off"] as const;
export type SourceId = (typeof SOURCES)[number];
export type SourceStateId = (typeof STATES)[number];

/** Tipos de evento que entram na linha do tempo (as amostras de contadores têm cartão próprio). */
export const TIMELINE_KINDS = ["HealthEvent", "PowerStatus", "InventoryChange"] as const;
export const SAMPLE_KIND = "HealthSample";

/** Cor/ênfase do estado: só "atenção" chama a atenção; indisponível, pausada e desligada são neutros (sem alarme falso). */
export function stateTone(state: string): "good" | "warn" | "neutral" {
  if (state === "ok") return "good";
  if (state === "attention") return "warn";
  return "neutral";
}

/** Fonte e estado conhecidos, na ordem fixa; algo desconhecido (versão nova do backend) é ignorado, não inventado. */
export function orderedSources(list: HealthSource[]): Array<{ source: SourceId; state: SourceStateId }> {
  const out: Array<{ source: SourceId; state: SourceStateId }> = [];
  for (const id of SOURCES) {
    const row = list.find((s) => s.source === id);
    if (row && (STATES as readonly string[]).includes(row.state)) out.push({ source: id, state: row.state as SourceStateId });
  }
  return out;
}

/** Separa as mudanças de inventário (lista própria) do resto da linha do tempo. Ambas mais recentes primeiro. */
export function splitTimeline(rows: ActivityRow[]): { timeline: ActivityRow[]; inventory: ActivityRow[] } {
  const byNewest = [...rows].sort((a, b) => b.tsUtcMs - a.tsUtcMs || b.seq - a.seq);
  return {
    timeline: byNewest.filter((r) => r.kind !== "InventoryChange" && (TIMELINE_KINDS as readonly string[]).includes(r.kind)),
    inventory: byNewest.filter((r) => r.kind === "InventoryChange"),
  };
}

/** O que NÃO é monitorado (chaves do dicionário, na ordem exibida). */
export const NOT_MONITORED = ["smart", "tpm", "wheaOperational", "sensors", "kernelDrivers", "admin"] as const;
/** O que nunca é gravado (chaves do dicionário). */
export const NEVER_RECORDED = ["messages", "identifiers", "names", "perProgram"] as const;
