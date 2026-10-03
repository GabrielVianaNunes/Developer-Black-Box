//! Consultas de leitura para o dashboard: atividade, processos, visão geral e incidentes.
//!
//! Só devolve campos da allowlist do modelo de eventos (pid, nome do executável,
//! números e o tipo do evento). Nada aqui grava dados nem contorna o Guard: lê apenas
//! o que o recorder já persistiu (segmentos selados e o journal ativo).

use std::collections::{BTreeMap, HashMap};

use bb_recorder::Recorder;
use bb_store::{Incident, Store};
use serde::Serialize;
use serde_json::Value;

/// Quantos segmentos selados (do mais novo para trás) as consultas varrem.
pub const DEFAULT_SEGMENT_SCAN: usize = 24;

#[derive(Clone, Debug)]
struct Ev {
    seq: u64,
    ts: i64,
    kind: String,
    body: Value,
}

fn parse_line(line: &str) -> Option<Ev> {
    let v: Value = serde_json::from_str(line).ok()?;
    let (kind, body) = v.get("kind")?.as_object()?.iter().next().map(|(k, b)| (k.clone(), b.clone()))?;
    Some(Ev { seq: v.get("seq")?.as_u64()?, ts: v.get("ts_utc_ms")?.as_i64()?, kind, body })
}

/// Eventos em ordem cronológica: últimos `max_segments` segmentos selados + journal ativo.
/// Segmentos ilegíveis são ignorados aqui; a verificação de integridade os reporta.
fn recent_events(rec: &Recorder, max_segments: usize) -> Vec<Ev> {
    let segs = rec.list_segments().unwrap_or_default();
    let start = segs.len().saturating_sub(max_segments);
    let mut out = Vec::new();
    for s in &segs[start..] {
        if let Ok(lines) = rec.read_segment(s.index) {
            out.extend(lines.iter().filter_map(|l| parse_line(l)));
        }
    }
    out.extend(rec.journal_lines().iter().filter_map(|l| parse_line(l)));
    out
}

fn key_of(body: &Value) -> Option<(u64, i64)> {
    let k = body.get("key")?;
    Some((k.get("pid")?.as_u64()?, k.get("start_time_ms")?.as_i64()?))
}

fn exe_map(events: &[Ev]) -> HashMap<(u64, i64), String> {
    events
        .iter()
        .filter(|e| e.kind == "ProcessStarted")
        .filter_map(|e| Some((key_of(&e.body)?, e.body.get("exe_name")?.as_str()?.to_owned())))
        .collect()
}

/// Detalhe de um evento, sem texto: só um código e números. A interface o formata no idioma
/// escolhido, e a exportação fica neutra em relação a idioma.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Detail {
    ProcessStarted { parent_pid: u64 },
    ProcessExited { exit_code: Option<i64> },
    ProcessMetrics { cpu_permille: u64, working_set_kb: u64 },
    SystemMetrics { cpu_permille: u64, mem_used_kb: u64, mem_total_kb: u64 },
    AppCrash { exception_code: u64 },
    AppHang,
    /// Evento de saúde da máquina: categoria (enumeração fechada), ID do evento e um número opcional (sem texto).
    HealthEvent { category: String, event_id: u64, value: Option<u64> },
    UserMarker { marker: u64 },
    RecorderStateChanged,
    Unknown,
}

#[derive(Clone, Debug, Default)]
pub struct ActivityFilter {
    /// Nomes de tipo (ex. "ProcessStarted"); vazio = todos.
    pub kinds: Vec<String>,
    /// Filtra pelo nome do executável (substring, sem diferenciar maiúsculas).
    pub text: String,
    pub from_utc_ms: Option<i64>,
    pub to_utc_ms: Option<i64>,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    pub seq: u64,
    pub ts_utc_ms: i64,
    pub kind: String,
    pub pid: Option<u32>,
    pub exe_name: Option<String>,
    pub detail: Detail,
}

fn row_of(e: &Ev, exes: &HashMap<(u64, i64), String>) -> ActivityRow {
    let key = key_of(&e.body);
    let exe_name = match e.kind.as_str() {
        "ProcessStarted" | "AppCrash" | "AppHang" => e.body.get("exe_name").and_then(Value::as_str).map(str::to_owned),
        _ => key.and_then(|k| exes.get(&k).cloned()),
    };
    let n = |k: &str| e.body.get(k).and_then(Value::as_u64).unwrap_or(0);
    let detail = match e.kind.as_str() {
        "ProcessStarted" => Detail::ProcessStarted { parent_pid: n("parent_pid") },
        "ProcessExited" => Detail::ProcessExited { exit_code: e.body.get("exit_code").and_then(Value::as_i64) },
        "ProcessMetrics" => {
            Detail::ProcessMetrics { cpu_permille: n("cpu_permille"), working_set_kb: n("working_set_kb") }
        }
        "SystemMetrics" => Detail::SystemMetrics {
            cpu_permille: n("cpu_permille"),
            mem_used_kb: n("mem_used_kb"),
            mem_total_kb: n("mem_total_kb"),
        },
        "AppCrash" => Detail::AppCrash { exception_code: n("exception_code") },
        "AppHang" => Detail::AppHang,
        "HealthEvent" => Detail::HealthEvent {
            category: e.body.get("category").and_then(Value::as_str).unwrap_or("").to_owned(),
            event_id: n("event_id"),
            value: e.body.get("code").and_then(Value::as_u64),
        },
        "UserMarker" => Detail::UserMarker { marker: n("code") },
        "RecorderStateChanged" => Detail::RecorderStateChanged,
        _ => Detail::Unknown,
    };
    ActivityRow {
        seq: e.seq,
        ts_utc_ms: e.ts,
        kind: e.kind.clone(),
        pid: key.map(|k| k.0 as u32),
        exe_name,
        detail,
    }
}

/// Atividade mais recente primeiro.
pub fn activity(rec: &Recorder, filter: &ActivityFilter) -> Vec<ActivityRow> {
    let events = recent_events(rec, DEFAULT_SEGMENT_SCAN);
    let exes = exe_map(&events);
    let needle = filter.text.trim().to_lowercase();
    let limit = filter.limit.clamp(1, 1000);
    events
        .iter()
        .rev()
        .filter(|e| filter.kinds.is_empty() || filter.kinds.contains(&e.kind))
        .filter(|e| filter.from_utc_ms.is_none_or(|t| e.ts >= t))
        .filter(|e| filter.to_utc_ms.is_none_or(|t| e.ts <= t))
        .map(|e| row_of(e, &exes))
        .filter(|r| needle.is_empty() || r.exe_name.as_ref().is_some_and(|n| n.to_lowercase().contains(&needle)))
        .take(limit)
        .collect()
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
    pub pid: u32,
    pub start_time_ms: i64,
    pub exe_name: String,
    pub parent_pid: u32,
    /// Conforme o último registro: com a gravação pausada este valor pode estar desatualizado.
    pub running: bool,
    pub ended_utc_ms: Option<i64>,
    pub cpu_permille: Option<u16>,
    pub working_set_kb: Option<u64>,
}

/// Instâncias vistas no período varrido: em execução primeiro (mais memória antes), depois as encerradas.
pub fn processes(rec: &Recorder, limit: usize) -> Vec<ProcessRow> {
    let events = recent_events(rec, DEFAULT_SEGMENT_SCAN);
    let mut by_key: BTreeMap<(u64, i64), ProcessRow> = BTreeMap::new();
    for e in &events {
        let Some(k) = key_of(&e.body) else { continue };
        match e.kind.as_str() {
            "ProcessStarted" => {
                let exe = e.body.get("exe_name").and_then(Value::as_str).unwrap_or("").to_owned();
                by_key.insert(
                    k,
                    ProcessRow {
                        pid: k.0 as u32,
                        start_time_ms: k.1,
                        exe_name: exe,
                        parent_pid: e.body.get("parent_pid").and_then(Value::as_u64).unwrap_or(0) as u32,
                        running: true,
                        ended_utc_ms: None,
                        cpu_permille: None,
                        working_set_kb: None,
                    },
                );
            }
            "ProcessMetrics" => {
                if let Some(p) = by_key.get_mut(&k) {
                    p.cpu_permille = e.body.get("cpu_permille").and_then(Value::as_u64).map(|v| v as u16);
                    p.working_set_kb = e.body.get("working_set_kb").and_then(Value::as_u64);
                }
            }
            "ProcessExited" => {
                if let Some(p) = by_key.get_mut(&k) {
                    p.running = false;
                    p.ended_utc_ms = Some(e.ts);
                }
            }
            _ => {}
        }
    }
    let mut rows: Vec<ProcessRow> = by_key.into_values().collect();
    rows.sort_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then(b.working_set_kb.unwrap_or(0).cmp(&a.working_set_kb.unwrap_or(0)))
            .then(b.ended_utc_ms.cmp(&a.ended_utc_ms))
    });
    rows.truncate(limit.clamp(1, 500));
    rows
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemRow {
    pub ts_utc_ms: i64,
    pub cpu_permille: u16,
    pub mem_used_kb: u64,
    pub mem_total_kb: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub storage_bytes: u64,
    pub sealed_segments: usize,
    pub incidents_total: usize,
    pub incidents_open: usize,
    pub latest_system: Option<SystemRow>,
    pub events_last_hour: u64,
    pub starts_last_hour: u64,
    pub exits_last_hour: u64,
}

pub fn overview(rec: &Recorder, store: Option<&Store>, now_utc_ms: i64) -> Overview {
    let events = recent_events(rec, DEFAULT_SEGMENT_SCAN);
    let since = now_utc_ms - 3_600_000;
    let recent = || events.iter().filter(|e| e.ts >= since);
    let latest_system = events.iter().rev().find(|e| e.kind == "SystemMetrics").map(|e| {
        let n = |k: &str| e.body.get(k).and_then(Value::as_u64).unwrap_or(0);
        SystemRow {
            ts_utc_ms: e.ts,
            cpu_permille: n("cpu_permille") as u16,
            mem_used_kb: n("mem_used_kb"),
            mem_total_kb: n("mem_total_kb"),
        }
    });
    let incidents = store.and_then(|s| s.list_incidents().ok()).unwrap_or_default();
    Overview {
        storage_bytes: rec.storage_bytes(),
        sealed_segments: rec.list_segments().map(|s| s.len()).unwrap_or(0),
        incidents_total: incidents.len(),
        incidents_open: incidents
            .iter()
            .filter(|i| matches!(i.state, bb_store::InvestigationState::New | bb_store::InvestigationState::Investigating))
            .count(),
        latest_system,
        events_last_hour: recent().count() as u64,
        starts_last_hour: recent().filter(|e| e.kind == "ProcessStarted").count() as u64,
        exits_last_hour: recent().filter(|e| e.kind == "ProcessExited").count() as u64,
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IncidentDto {
    pub id: i64,
    pub kind: String,
    pub severity: String,
    pub created_utc_ms: i64,
    pub exe_name: Option<String>,
    pub summary: String,
    pub state: String,
    pub capture: String,
    pub post_until_utc_ms: i64,
    pub segments: Vec<u64>,
}

impl From<&Incident> for IncidentDto {
    fn from(i: &Incident) -> Self {
        IncidentDto {
            id: i.id,
            kind: i.kind.as_str().into(),
            severity: i.severity.as_str().into(),
            created_utc_ms: i.created_utc_ms,
            exe_name: i.exe_name.clone(),
            summary: i.summary.clone(),
            state: i.state.as_str().into(),
            capture: i.capture.as_str().into(),
            post_until_utc_ms: i.post_until_utc_ms,
            segments: i.segments.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteDto {
    pub id: i64,
    pub created_utc_ms: i64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRow {
    pub offset_ms: i64,
    #[serde(flatten)]
    pub row: ActivityRow,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SegmentDto {
    pub index: u64,
    pub size: u64,
    pub preserved: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IncidentDetail {
    pub incident: IncidentDto,
    pub notes: Vec<NoteDto>,
    pub segments: Vec<SegmentDto>,
    /// Eventos dos segmentos de evidência, em ordem cronológica (no máximo 1000, os mais próximos do fim).
    pub timeline: Vec<TimelineRow>,
}

/// Regras de privacidade VIGENTES no momento da exportação (não as de quando o dado foi gravado).
#[derive(Clone, Debug, Default)]
pub struct ExportRules {
    pub excluded: std::collections::HashSet<String>,
    pub protected: std::collections::HashSet<String>,
    /// Apps protegidos com autorização de teste ativa agora; só estes saem mesmo sendo protegidos.
    pub authorized_now: std::collections::HashSet<String>,
    /// Exclusões parciais vigentes: deste programa, só estes tipos de evento saem da exportação.
    pub partial: std::collections::HashMap<String, bb_core::ExclusionSet>,
    /// Programas excluídos por inteiro cuja exclusão vale também para o que eles iniciaram (filhos, netos...).
    pub trees: std::collections::HashSet<String>,
}

impl ExportRules {
    /// Eventos deste programa e deste tipo podem sair agora?
    fn allows(&self, exe: &str, kind: bb_core::ExclusionKind) -> bool {
        !self.excluded.contains(exe)
            && !self.partial.get(exe).is_some_and(|s| s.contains(kind))
            && (!self.protected.contains(exe) || self.authorized_now.contains(exe))
    }

    /// O NOME do programa pode aparecer (por exemplo no resumo do incidente)? Se o programa tem qualquer
    /// exclusão, total ou parcial, o nome some: é o lado mais privado.
    fn allows_name(&self, exe: &str) -> bool {
        !self.excluded.contains(exe)
            && !self.partial.contains_key(exe)
            && (!self.protected.contains(exe) || self.authorized_now.contains(exe))
    }
}

/// Instâncias (pid, início) que descendem de um programa excluído com a opção dos filhos, reconstruídas dos inícios
/// gravados (nome, pid do pai e horário). Um filho só descende de quem começou ANTES dele (PID reaproveitado). Eventos
/// de falha e travamento só trazem o nome do programa, então não podem ser ligados à árvore.
fn tree_members(events: &[Ev], roots: &std::collections::HashSet<String>) -> std::collections::HashSet<(u64, i64)> {
    let mut members = std::collections::HashSet::new();
    if roots.is_empty() {
        return members;
    }
    let starts: Vec<((u64, i64), String, u64)> = events
        .iter()
        .filter(|e| e.kind == "ProcessStarted")
        .filter_map(|e| {
            let exe = e.body.get("exe_name")?.as_str()?.to_owned();
            Some((key_of(&e.body)?, exe, e.body.get("parent_pid").and_then(Value::as_u64).unwrap_or(0)))
        })
        .collect();
    loop {
        let before = members.len();
        for (key, exe, parent) in &starts {
            if members.contains(key) {
                continue;
            }
            let descends = *parent != 0 && members.iter().any(|(pid, start): &(u64, i64)| pid == parent && *start <= key.1);
            if roots.contains(exe) || descends {
                members.insert(*key);
            }
        }
        if members.len() == before {
            break;
        }
    }
    members
}

/// Tipo de exclusão a que um tipo de evento gravado pertence (`None`: não é evento de um programa).
fn exclusion_kind_of(event: &str) -> Option<bb_core::ExclusionKind> {
    use bb_core::ExclusionKind::*;
    match event {
        "ProcessStarted" | "ProcessExited" => Some(Lifecycle),
        "ProcessMetrics" => Some(Metrics),
        "AppCrash" | "AppHang" => Some(Crashes),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportDoc {
    pub format: &'static str,
    pub exported_at_utc_ms: i64,
    pub incident: IncidentDto,
    pub events: Vec<TimelineRow>,
    /// Eventos removidos pela nova filtragem (o conteúdo deles não é citado).
    pub dropped_events: usize,
    /// Anotações são texto privado seu e nunca entram na exportação.
    pub notes_included: bool,
}

/// Exporta as evidências de um incidente APLICANDO DE NOVO as regras de privacidade atuais.
/// Falha fechada: eventos de processo cujo início não está na evidência (app desconhecido) saem.
pub fn export_incident(rec: &Recorder, store: &Store, id: i64, rules: &ExportRules, now_utc_ms: i64) -> Option<ExportDoc> {
    let inc = store.get_incident(id).ok()??;
    let mut events = Vec::new();
    for idx in &inc.segments {
        if let Ok(lines) = rec.read_segment(*idx) {
            events.extend(lines.iter().filter_map(|l| parse_line(l)));
        }
    }
    events.sort_by_key(|e| (e.ts, e.seq));
    let exes = exe_map(&events);
    let tree = tree_members(&events, &rules.trees);

    let total = events.len();
    let mut kept = Vec::new();
    for e in &events {
        // Início, CPU/memória e fim de quem está na árvore de um programa excluído nunca saem.
        if matches!(e.kind.as_str(), "ProcessStarted" | "ProcessMetrics" | "ProcessExited") && key_of(&e.body).is_some_and(|k| tree.contains(&k)) {
            continue;
        }
        let row = row_of(e, &exes);
        let keep = match e.kind.as_str() {
            // Eventos de um app: precisam de um nome conhecido e permitido agora.
            "ProcessStarted" | "ProcessMetrics" | "ProcessExited" | "AppCrash" | "AppHang" => match exclusion_kind_of(e.kind.as_str()) {
                Some(kind) => row.exe_name.as_deref().is_some_and(|n| rules.allows(n, kind)),
                None => false,
            },
            "SystemMetrics" | "UserMarker" => true,
            _ => false,
        };
        if keep {
            kept.push(TimelineRow { offset_ms: e.ts - inc.created_utc_ms, row });
        }
    }

    let mut dto = IncidentDto::from(&inc);
    if dto.exe_name.as_deref().is_some_and(|n| !rules.allows_name(n)) {
        dto.exe_name = None;
    }
    Some(ExportDoc {
        format: "developer-blackbox-export/1",
        exported_at_utc_ms: now_utc_ms,
        incident: dto,
        dropped_events: total - kept.len(),
        events: kept,
        notes_included: false,
    })
}

/// Uma linha do tempo mostra fatos na ordem em que foram gravados; não sugere causa.
pub fn incident_detail(rec: &Recorder, store: &Store, id: i64) -> Option<IncidentDetail> {
    let inc = store.get_incident(id).ok()??;
    let notes = store
        .list_notes(id)
        .unwrap_or_default()
        .into_iter()
        .map(|n| NoteDto { id: n.id, created_utc_ms: n.created_utc_ms, text: n.text })
        .collect();

    let all = rec.list_segments().unwrap_or_default();
    let segments: Vec<SegmentDto> = all
        .iter()
        .filter(|s| inc.segments.contains(&s.index))
        .map(|s| SegmentDto { index: s.index, size: s.size, preserved: s.preserved })
        .collect();

    let mut events = Vec::new();
    for s in &segments {
        if let Ok(lines) = rec.read_segment(s.index) {
            events.extend(lines.iter().filter_map(|l| parse_line(l)));
        }
    }
    events.sort_by_key(|e| (e.ts, e.seq));
    let exes = exe_map(&events);
    let skip = events.len().saturating_sub(1000);
    let timeline = events[skip..]
        .iter()
        .map(|e| TimelineRow { offset_ms: e.ts - inc.created_utc_ms, row: row_of(e, &exes) })
        .collect();

    Some(IncidentDetail { incident: IncidentDto::from(&inc), notes, segments, timeline })
}
