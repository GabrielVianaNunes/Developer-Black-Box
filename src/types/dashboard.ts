/** Espelham os DTOs do backend. Só campos do modelo de eventos; nunca conteúdo. */

export interface ActivityRow {
  seq: number;
  tsUtcMs: number;
  kind: string;
  pid: number | null;
  exeName: string | null;
  detail: string;
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

export interface Settings {
  protectedApps: string[];
  excludedApps: string[];
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
