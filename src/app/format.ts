export const fmtTime = (ms: number) => new Date(ms).toLocaleString("pt-BR");
export const fmtClock = (ms: number) => new Date(ms).toLocaleTimeString("pt-BR");

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

export const fmtKb = (kb: number) => fmtBytes(kb * 1024);
export const fmtPct = (permille: number) => `${(permille / 10).toFixed(1)}%`;

/** Deslocamento em relação ao momento do incidente: -00:03.5 / +00:12.0 */
export function fmtOffset(ms: number): string {
  const sign = ms < 0 ? "-" : "+";
  const abs = Math.abs(ms);
  const s = Math.floor(abs / 1000);
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${sign}${mm}:${ss}.${Math.floor((abs % 1000) / 100)}`;
}

export const KIND_LABEL: Record<string, string> = {
  ProcessStarted: "Início de processo",
  ProcessExited: "Fim de processo",
  ProcessMetrics: "Métricas de processo",
  AppCrash: "Falha de aplicativo",
  AppHang: "Aplicativo sem resposta",
  SystemMetrics: "Métricas do sistema",
  RecorderStateChanged: "Estado do recorder",
  UserMarker: "Marcador",
};

export const INCIDENT_KIND_LABEL: Record<string, string> = {
  manual: "Captura manual",
  cpu_sustained: "CPU sustentada",
  memory_high: "Memória alta",
  unexpected_exit: "Encerramento inesperado",
  app_hang: "Aplicativo sem resposta",
};

export const SEVERITY_LABEL: Record<string, string> = { info: "Informativo", warning: "Atenção", critical: "Crítico" };

export const INVESTIGATION_LABEL: Record<string, string> = {
  new: "Novo",
  investigating: "Investigando",
  resolved: "Resolvido",
  dismissed: "Descartado",
};

export const CONFIG_KEY_LABEL: Record<string, string> = {
  protected_apps: "Aplicativos protegidos",
  excluded_apps: "Aplicativos excluídos",
  stability_window_ms: "Janela de estabilidade",
  auto_start: "Início automático",
  retention_max_mb: "Limite de armazenamento",
  retention_max_hours: "Retenção",
  authorizations: "Autorizações de teste",
  launch_at_login: "Abrir com o Windows",
};

export function fmtRemaining(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h} h ${String(m).padStart(2, "0")} min` : `${m} min ${String(s % 60).padStart(2, "0")} s`;
}

export const CONFIG_CHANGE_LABEL: Record<string, string> = { added: "adicionado", removed: "removido", changed: "alterado" };
