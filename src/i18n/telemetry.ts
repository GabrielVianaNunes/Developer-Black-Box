/** Resume uma amostra de contadores de desempenho do sistema (só números). Puro: recebe a função de tradução. */

type T = (key: string, params?: Record<string, string | number>) => string;

export interface SampleFields {
  thermalKelvin: number | null;
  passiveLimitPct: number | null;
  cpuLoadPct: number | null;
  cpuPerfPct: number | null;
  cpuFreqMhz: number | null;
  memCommitPct: number | null;
  memAvailableMb: number | null;
  pageFaultsPerSec: number | null;
  diskLatencyUs: number | null;
  diskBusyPct: number | null;
  netErrors: number | null;
  gpuPct: number | null;
}

/** Só os contadores que existem aparecem; os indisponíveis ficam de fora (nada de "0" inventado). */
export function sampleText(s: SampleFields, t: T): string {
  const parts: string[] = [];
  if (s.thermalKelvin != null) parts.push(t("sample.temp", { c: Math.round(s.thermalKelvin - 273.15) }));
  if (s.passiveLimitPct != null) parts.push(t("sample.limit", { pct: s.passiveLimitPct }));
  if (s.cpuLoadPct != null) parts.push(t("sample.cpu", { pct: s.cpuLoadPct }));
  if (s.cpuFreqMhz != null) parts.push(t("sample.freq", { mhz: s.cpuFreqMhz }));
  if (s.memCommitPct != null) parts.push(t("sample.commit", { pct: s.memCommitPct }));
  if (s.memAvailableMb != null) parts.push(t("sample.available", { mb: s.memAvailableMb }));
  if (s.pageFaultsPerSec != null) parts.push(t("sample.faults", { n: s.pageFaultsPerSec }));
  if (s.diskLatencyUs != null) parts.push(t("sample.latency", { ms: (s.diskLatencyUs / 1000).toFixed(1) }));
  if (s.diskBusyPct != null) parts.push(t("sample.disk", { pct: s.diskBusyPct }));
  if (s.netErrors != null) parts.push(t("sample.neterr", { n: s.netErrors }));
  if (s.gpuPct != null) parts.push(t("sample.gpu", { pct: s.gpuPct }));
  return parts.length ? parts.join(" · ") : t("sample.none");
}
