/** Espelham os DTOs do backend. Só campos do modelo de eventos; nunca conteúdo. */

/** Detalhe de um evento: um código e números, sem texto de idioma (a interface o formata). */
export type Detail =
  | { code: "processStarted"; parentPid: number }
  | { code: "processExited"; exitCode: number | null }
  | { code: "processMetrics"; cpuPermille: number; workingSetKb: number }
  | { code: "systemMetrics"; cpuPermille: number; memUsedKb: number; memTotalKb: number }
  | { code: "appCrash"; exceptionCode: number }
  | { code: "appHang" }
  | { code: "userMarker"; marker: number }
  | { code: "recorderStateChanged" }
  | { code: "unknown" };

export interface ActivityRow {
  seq: number;
  tsUtcMs: number;
  kind: string;
  pid: number | null;
  exeName: string | null;
  detail: Detail;
}

export interface ActivityFilter {
  kinds: string[];
  text: string;
  fromUtcMs: number | null;
  toUtcMs: number | null;
  limit: number;
}

export interface ProcessRow {
  pid: number;
  startTimeMs: number;
  exeName: string;
  parentPid: number;
  running: boolean;
  endedUtcMs: number | null;
  cpuPermille: number | null;
  workingSetKb: number | null;
}

export interface SystemRow {
  tsUtcMs: number;
  cpuPermille: number;
  memUsedKb: number;
  memTotalKb: number;
}

export interface Overview {
  storageBytes: number;
  sealedSegments: number;
  incidentsTotal: number;
  incidentsOpen: number;
  latestSystem: SystemRow | null;
  eventsLastHour: number;
  startsLastHour: number;
  exitsLastHour: number;
}

export interface Incident {
  id: number;
  kind: string;
  severity: string;
  createdUtcMs: number;
  exeName: string | null;
  summary: string;
  state: string;
  capture: string;
  postUntilUtcMs: number;
  segments: number[];
}

export interface Note {
  id: number;
  createdUtcMs: number;
  text: string;
}

export interface Segment {
  index: number;
  size: number;
  preserved: boolean;
}

export type TimelineRow = ActivityRow & { offsetMs: number };

export interface IncidentDetail {
  incident: Incident;
  notes: Note[];
  segments: Segment[];
  timeline: TimelineRow[];
}

/** Tipos de evento que uma exclusão parcial pode cobrir (códigos estáveis do backend). */
export type ExclusionKind = "lifecycle" | "metrics" | "crashes";

/** Do programa `exe`, os tipos em `kinds` NÃO são gravados; o resto continua sendo. */
export interface PartialExclusion {
  exe: string;
  kinds: ExclusionKind[];
}

export interface Settings {
  protectedApps: string[];
  excludedApps: string[];
  /** Exclusões por tipo de evento. Precisa voltar intacto ao salvar: o backend recusa um cliente que o omita. */
  partialExclusions: PartialExclusion[];
  stabilityWindowMs: number;
  autoStart: boolean;
  retentionMaxMb: number;
  retentionMaxHours: number;
}

export interface ExportResult {
  path: string;
  events: number;
  dropped: number;
}

export interface Authorization {
  exe: string;
  remainingMs: number;
  allowMetrics: boolean;
  allowCrashes: boolean;
}

export interface ConfigChange {
  atUtcMs: number;
  key: string;
  change: string;
}

export interface Storage {
  storageBytes: number;
  maxTotalBytes: number;
  maxAgeHours: number;
  segments: Segment[];
}

export interface Verify {
  ok: boolean;
  segments: number;
  events: number;
  error: string | null;
}

/** Programa deste PC que pode ser escolhido nas listas de privacidade (só existe na memória da tela). */
export interface AppCandidate {
  exe: string;
  name: string;
  running: boolean;
  installed: boolean;
}

/** Resultado de "Detectar o app em primeiro plano": só o nome do executável e se era o próprio app. */
export interface Detection {
  exe: string | null;
  isSelf: boolean;
}
