//! Consultas de leitura para o dashboard: atividade, processos, visão geral e incidentes.
//!
//! Só devolve campos da allowlist do modelo de eventos (pid, nome do executável,
//! números e o tipo do evento). Nada aqui grava dados nem contorna o Guard: lê apenas
//! o que o recorder já persistiu (segmentos selados e o journal ativo).

use std::collections::{BTreeMap, HashMap};

mod summary;
pub use summary::{bug_report_summary, SummaryLang};

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
    ProcessStarted {
        parent_pid: u64,
    },
    ProcessExited {
        exit_code: Option<i64>,
    },
    ProcessMetrics {
        cpu_permille: u64,
        working_set_kb: u64,
    },
    SystemMetrics {
        cpu_permille: u64,
        mem_used_kb: u64,
        mem_total_kb: u64,
    },
    AppCrash {
        exception_code: u64,
    },
    AppHang,
    /// Evento de saúde da máquina: categoria (enumeração fechada), ID do evento e um número opcional (sem texto).
    HealthEvent {
        category: String,
        event_id: u64,
        value: Option<u64>,
    },
    /// Mudança de inventário: item (enumeração fechada) e valores numéricos anterior e novo.
    InventoryChange {
        item: String,
        previous: Option<u64>,
        current: Option<u64>,
    },
    /// Energia: tomada ("offline" ou "online"; ausente = desconhecida) e carga em porcentagem.
    PowerStatus {
        ac: Option<String>,
        charge_percent: Option<u64>,
    },
    /// Amostra de contadores de desempenho do sistema: só números; cada campo ausente = indisponível.
    HealthSample {
        thermal_kelvin: Option<u64>,
        passive_limit_pct: Option<u64>,
        cpu_load_pct: Option<u64>,
        cpu_perf_pct: Option<u64>,
        cpu_freq_mhz: Option<u64>,
        mem_commit_pct: Option<u64>,
        mem_available_mb: Option<u64>,
        page_faults_per_sec: Option<u64>,
        disk_latency_us: Option<u64>,
        disk_busy_pct: Option<u64>,
        net_errors: Option<u64>,
        gpu_pct: Option<u64>,
    },
    UserMarker {
        marker: u64,
    },
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
        "ProcessMetrics" => Detail::ProcessMetrics { cpu_permille: n("cpu_permille"), working_set_kb: n("working_set_kb") },
        "SystemMetrics" => {
            Detail::SystemMetrics { cpu_permille: n("cpu_permille"), mem_used_kb: n("mem_used_kb"), mem_total_kb: n("mem_total_kb") }
        }
        "AppCrash" => Detail::AppCrash { exception_code: n("exception_code") },
        "AppHang" => Detail::AppHang,
        "HealthEvent" => Detail::HealthEvent {
            category: e.body.get("category").and_then(Value::as_str).unwrap_or("").to_owned(),
            event_id: n("event_id"),
            value: e.body.get("code").and_then(Value::as_u64),
        },
        "InventoryChange" => Detail::InventoryChange {
            item: e.body.get("item").and_then(Value::as_str).unwrap_or("").to_owned(),
            previous: e.body.get("previous").and_then(Value::as_u64),
            current: e.body.get("current").and_then(Value::as_u64),
        },
        "PowerStatus" => Detail::PowerStatus {
            ac: e.body.get("ac").and_then(Value::as_str).map(|s| s.to_lowercase()),
            charge_percent: e.body.get("charge_percent").and_then(Value::as_u64),
        },
        "HealthSample" => {
            let o = |k: &str| e.body.get(k).and_then(Value::as_u64);
            Detail::HealthSample {
                thermal_kelvin: o("thermal_kelvin"),
                passive_limit_pct: o("passive_limit_pct"),
                cpu_load_pct: o("cpu_load_pct"),
                cpu_perf_pct: o("cpu_perf_pct"),
                cpu_freq_mhz: o("cpu_freq_mhz"),
                mem_commit_pct: o("mem_commit_pct"),
                mem_available_mb: o("mem_available_mb"),
                page_faults_per_sec: o("page_faults_per_sec"),
                disk_latency_us: o("disk_latency_us"),
                disk_busy_pct: o("disk_busy_pct"),
                net_errors: o("net_errors"),
                gpu_pct: o("gpu_pct"),
            }
        }
        "UserMarker" => Detail::UserMarker { marker: n("code") },
        "RecorderStateChanged" => Detail::RecorderStateChanged,
        _ => Detail::Unknown,
    };
    ActivityRow { seq: e.seq, ts_utc_ms: e.ts, kind: e.kind.clone(), pid: key.map(|k| k.0 as u32), exe_name, detail }
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
    /// Janela de evidência anterior ao incidente (ms): a janela de saúde da exportação começa ali.
    pub pre_window_ms: i64,
    /// As amostras de contadores de desempenho só saem se a telemetria estiver LIGADA agora (padrão fechado: não saem).
    pub include_samples: bool,
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
    /// A saúde da máquina na janela do incidente, sempre refiltrada. Formato em `HealthExport`.
    pub health: HealthExport,
}

/// A saúde da máquina (eventos do Windows, mudanças de inventário, energia e amostras de desempenho) na janela do
/// incidente. Só números e enumerações fechadas: cada linha é conferida de novo na exportação contra os conjuntos de
/// valores permitidos, e o que não bater é descartado.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthExport {
    /// Início e fim da janela (ms UTC). Só entra o que tem horário dentro dela.
    pub from_utc_ms: i64,
    pub to_utc_ms: i64,
    /// Linhas em ordem cronológica, com `offsetMs` em relação ao horário do incidente.
    pub events: Vec<TimelineRow>,
    /// Quantas linhas de saúde da janela ficaram de fora (falharam a conferência ou as regras de hoje). Sem citar conteúdo.
    pub dropped: usize,
    /// Havia mais linhas do que o teto (`MAX_HEALTH_ROWS`): ficaram as mais próximas do incidente.
    pub truncated: bool,
}

/// Teto de linhas de saúde por exportação (um dia de amostras a cada 30 s são 2.880).
pub const MAX_HEALTH_ROWS: usize = 5_000;
/// A janela de saúde não recua mais que isto antes do incidente (o limite da leitura do que ocorreu com o app fechado).
const MAX_HEALTH_LOOKBACK_MS: i64 = 8 * 86_400_000;

const HEALTH_CATEGORIES: &[&str] = &[
    "UnexpectedShutdown",
    "BugCheck",
    "HardwareError",
    "DisplayDriverReset",
    "DiskError",
    "FileSystemError",
    "ServiceCrash",
    "UpdateFailure",
    "SleepEntered",
    "Resumed",
];
const INVENTORY_ITEMS: &[&str] =
    &["BiosVersion", "BiosDate", "FirmwareType", "SecureBoot", "OsBuild", "DeviceProblemCount", "DeviceProblemCodes"];
const AC_VALUES: &[&str] = &["Offline", "Online"];
const SAMPLE_KEYS: &[&str] = &[
    "thermal_kelvin",
    "passive_limit_pct",
    "cpu_load_pct",
    "cpu_perf_pct",
    "cpu_freq_mhz",
    "mem_commit_pct",
    "mem_available_mb",
    "page_faults_per_sec",
    "disk_latency_us",
    "disk_busy_pct",
    "net_errors",
    "gpu_pct",
];
/// Tudo é abaixo de 2^53: a interface (JavaScript) lê sem perder precisão.
const MAX_SAFE_NUMBER: u64 = 1 << 53;

fn is_kind_of_health(kind: &str) -> bool {
    matches!(kind, "HealthEvent" | "InventoryChange" | "PowerStatus" | "HealthSample")
}

/// Um campo que só pode ser número (ou ausente) dentro do limite.
fn number_or_null(body: &Value, key: &str, max: u64) -> bool {
    match body.get(key) {
        None => false, // o campo existe sempre; ausente = linha estranha
        Some(Value::Null) => true,
        Some(v) => v.as_u64().is_some_and(|n| n <= max),
    }
}

/// Um campo de texto que só pode ser um dos valores fechados.
fn one_of(body: &Value, key: &str, allowed: &[&str]) -> bool {
    body.get(key).and_then(Value::as_str).is_some_and(|s| allowed.contains(&s))
}

/// A linha de saúde tem EXATAMENTE as chaves esperadas, cada uma com um valor dos conjuntos fechados ou um número
/// dentro do limite? Qualquer chave a mais (um possível campo de texto), texto fora do conjunto ou número fora do
/// limite faz a linha ser descartada. As regras de hoje também entram: amostras só com a telemetria ligada.
fn health_row_allowed(kind: &str, body: &Value, rules: &ExportRules) -> bool {
    let Some(obj) = body.as_object() else { return false };
    let only_keys = |keys: &[&str]| obj.len() == keys.len() && keys.iter().all(|k| obj.contains_key(*k));
    match kind {
        "HealthEvent" => {
            only_keys(&["category", "event_id", "code"])
                && one_of(body, "category", HEALTH_CATEGORIES)
                && number_or_null(body, "code", u64::from(u32::MAX))
                && body.get("event_id").and_then(Value::as_u64).is_some_and(|n| n <= u64::from(u16::MAX))
        }
        "InventoryChange" => {
            only_keys(&["item", "previous", "current"])
                && one_of(body, "item", INVENTORY_ITEMS)
                && number_or_null(body, "previous", MAX_SAFE_NUMBER)
                && number_or_null(body, "current", MAX_SAFE_NUMBER)
        }
        "PowerStatus" => {
            only_keys(&["ac", "charge_percent"])
                && (body.get("ac").is_some_and(Value::is_null) || one_of(body, "ac", AC_VALUES))
                && number_or_null(body, "charge_percent", 100)
        }
        "HealthSample" => {
            rules.include_samples && only_keys(SAMPLE_KEYS) && SAMPLE_KEYS.iter().all(|k| number_or_null(body, k, u64::from(u32::MAX)))
        }
        _ => false,
    }
}

/// Quando a condição de um incidente de saúde começou, gravado como o último número do resumo (ms UTC). Só vale para os
/// tipos de saúde, só se o número for plausível (nunca depois do incidente, nem mais de 8 dias antes).
fn health_since(inc: &Incident) -> Option<i64> {
    use bb_store::IncidentKind::*;
    if !matches!(inc.kind, UnexpectedShutdown | BlueScreen | HardwareError | Throttling) {
        return None;
    }
    let since: i64 = inc.summary.rsplit('|').next()?.parse().ok()?;
    (since <= inc.created_utc_ms && inc.created_utc_ms - since <= MAX_HEALTH_LOOKBACK_MS).then_some(since)
}

/// Lê, dos segmentos que tocam a janela e do journal ativo, só as linhas de saúde com horário dentro dela.
fn health_events_in(rec: &Recorder, from: i64, to: i64) -> Vec<Ev> {
    let mut out = Vec::new();
    let mut take = |lines: Vec<String>| {
        out.extend(lines.iter().filter_map(|l| parse_line(l)).filter(|e| is_kind_of_health(&e.kind) && e.ts >= from && e.ts <= to));
    };
    for idx in rec.segments_overlapping(from, to).unwrap_or_default() {
        if let Ok(lines) = rec.read_segment(idx) {
            take(lines);
        }
    }
    take(rec.journal_lines());
    // Um evento está ou num segmento selado ou no journal ativo, nunca nos dois; o `seq` recomeça a cada execução do app,
    // por isso não serve para deduplicar.
    out.sort_by_key(|e| (e.ts, e.seq));
    out
}

fn health_export(rec: &Recorder, inc: &Incident, rules: &ExportRules) -> HealthExport {
    let pre = rules.pre_window_ms.max(0);
    let from = health_since(inc).map_or(inc.created_utc_ms, |s| s.min(inc.created_utc_ms)) - pre;
    let to = inc.post_until_utc_ms;
    let candidates = health_events_in(rec, from, to);
    let total = candidates.len();
    let exes = HashMap::new();
    let rows: Vec<TimelineRow> = candidates
        .iter()
        .filter(|e| health_row_allowed(&e.kind, &e.body, rules))
        .map(|e| TimelineRow { offset_ms: e.ts - inc.created_utc_ms, row: row_of(e, &exes) })
        .collect();
    let dropped = total - rows.len();
    let (events, truncated) = cap_rows(rows, MAX_HEALTH_ROWS);
    HealthExport { from_utc_ms: from, to_utc_ms: to, events, dropped, truncated }
}

/// No máximo `max` linhas: se passar, ficam as mais próximas do incidente, de volta em ordem cronológica.
fn cap_rows(mut rows: Vec<TimelineRow>, max: usize) -> (Vec<TimelineRow>, bool) {
    if rows.len() <= max {
        return (rows, false);
    }
    rows.sort_by_key(|r| (r.offset_ms.unsigned_abs(), r.row.seq));
    rows.truncate(max);
    rows.sort_by_key(|r| (r.row.ts_utc_ms, r.row.seq));
    (rows, true)
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

    // As linhas de saúde têm a própria seção (`health`, com janela e conferência próprias): aqui não contam nem como mantidas
    // nem como removidas.
    events.retain(|e| !is_kind_of_health(&e.kind));
    let total = events.len();
    let mut kept = Vec::new();
    for e in &events {
        // Início, CPU/memória e fim de quem está na árvore de um programa excluído nunca saem.
        if matches!(e.kind.as_str(), "ProcessStarted" | "ProcessMetrics" | "ProcessExited")
            && key_of(&e.body).is_some_and(|k| tree.contains(&k))
        {
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
        format: "developer-blackbox-export/2",
        exported_at_utc_ms: now_utc_ms,
        incident: dto,
        dropped_events: total - kept.len(),
        events: kept,
        notes_included: false,
        health: health_export(rec, &inc, rules),
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
    let timeline = events[skip..].iter().map(|e| TimelineRow { offset_ms: e.ts - inc.created_utc_ms, row: row_of(e, &exes) }).collect();

    Some(IncidentDetail { incident: IncidentDto::from(&inc), notes, segments, timeline })
}

#[cfg(test)]
mod health_export_tests {
    use super::*;
    use serde_json::json;

    fn rules(samples: bool) -> ExportRules {
        ExportRules { include_samples: samples, pre_window_ms: 60_000, ..ExportRules::default() }
    }

    fn sample_body() -> Value {
        let mut o = serde_json::Map::new();
        for k in SAMPLE_KEYS {
            o.insert((*k).to_owned(), json!(5));
        }
        Value::Object(o)
    }

    #[test]
    fn well_formed_rows_of_each_health_kind_pass() {
        let r = rules(true);
        assert!(health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":1001,"code":209}), &r));
        assert!(health_row_allowed("HealthEvent", &json!({"category":"Resumed","event_id":107,"code":null}), &r));
        assert!(health_row_allowed("InventoryChange", &json!({"item":"OsBuild","previous":1,"current":null}), &r));
        assert!(health_row_allowed("PowerStatus", &json!({"ac":"Online","charge_percent":80}), &r));
        assert!(health_row_allowed("PowerStatus", &json!({"ac":null,"charge_percent":null}), &r));
        assert!(health_row_allowed("HealthSample", &sample_body(), &r));
    }

    #[test]
    fn an_extra_key_is_never_allowed_because_it_could_be_free_text() {
        let r = rules(true);
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":1,"code":1,"note":"synth text"}), &r));
        assert!(!health_row_allowed("InventoryChange", &json!({"item":"OsBuild","previous":1,"current":2,"name":"synth"}), &r));
        assert!(!health_row_allowed("PowerStatus", &json!({"ac":"Online","charge_percent":1,"ssid":"synth"}), &r));
        let mut s = sample_body();
        s.as_object_mut().unwrap().insert("adapter".into(), json!("synth"));
        assert!(!health_row_allowed("HealthSample", &s, &r));
    }

    #[test]
    fn a_missing_key_is_not_allowed_either() {
        let r = rules(true);
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":1}), &r));
        assert!(!health_row_allowed("PowerStatus", &json!({"ac":"Online"}), &r));
    }

    #[test]
    fn text_outside_the_closed_sets_is_not_allowed() {
        let r = rules(true);
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"C:\\Users\\synth","event_id":1,"code":1}), &r));
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"bugcheck","event_id":1,"code":1}), &r), "case matters");
        assert!(!health_row_allowed("InventoryChange", &json!({"item":"SerialNumber","previous":1,"current":2}), &r));
        assert!(!health_row_allowed("PowerStatus", &json!({"ac":"HomeWifi","charge_percent":1}), &r));
    }

    #[test]
    fn a_number_field_holding_text_or_out_of_range_is_not_allowed() {
        let r = rules(true);
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":1,"code":"abc"}), &r));
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":70_000,"code":1}), &r));
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":1,"code":4_294_967_296u64}), &r));
        assert!(!health_row_allowed("HealthEvent", &json!({"category":"BugCheck","event_id":1,"code":-1}), &r));
        assert!(!health_row_allowed("InventoryChange", &json!({"item":"OsBuild","previous":9_007_199_254_740_993u64,"current":1}), &r));
        assert!(!health_row_allowed("PowerStatus", &json!({"ac":"Online","charge_percent":101}), &r));
        let mut s = sample_body();
        s["gpu_pct"] = json!("7");
        assert!(!health_row_allowed("HealthSample", &s, &r));
        s["gpu_pct"] = json!(1.5);
        assert!(!health_row_allowed("HealthSample", &s, &r));
    }

    #[test]
    fn samples_need_the_counters_to_be_on_today_and_other_rows_do_not() {
        let off = rules(false);
        assert!(!health_row_allowed("HealthSample", &sample_body(), &off));
        assert!(health_row_allowed("PowerStatus", &json!({"ac":"Online","charge_percent":10}), &off));
    }

    #[test]
    fn unknown_kinds_and_non_objects_are_never_allowed() {
        let r = rules(true);
        assert!(!health_row_allowed("ProcessStarted", &json!({}), &r));
        assert!(!health_row_allowed("SomethingNew", &json!({"a":1}), &r));
        assert!(!health_row_allowed("HealthEvent", &json!("text"), &r));
        assert!(!health_row_allowed("HealthEvent", &json!([1, 2]), &r));
    }

    fn incident(kind: bb_store::IncidentKind, summary: &str, created: i64) -> Incident {
        Incident {
            id: 1,
            kind,
            severity: bb_store::Severity::Critical,
            created_utc_ms: created,
            exe_name: None,
            summary: summary.into(),
            state: bb_store::InvestigationState::New,
            capture: bb_store::CaptureState::Preserved,
            post_until_utc_ms: created + 30_000,
            segments: vec![],
        }
    }

    #[test]
    fn the_start_of_the_condition_comes_only_from_a_health_incident_with_a_plausible_number() {
        use bb_store::IncidentKind::*;
        let c = 10_000_000_000;
        assert_eq!(health_since(&incident(BlueScreen, "blue_screen|209|9999000000", c)), Some(9_999_000_000));
        assert_eq!(health_since(&incident(Throttling, "throttling|70|95|9999700000", c)), Some(9_999_700_000));
        assert_eq!(health_since(&incident(CpuSustained, "cpu_sustained|900|3|9999000000", c)), None, "not a health incident");
        assert_eq!(health_since(&incident(BlueScreen, "blue_screen|209|10000000001", c)), None, "after the incident: implausible");
        assert_eq!(health_since(&incident(BlueScreen, "blue_screen|209|1", c)), None, "more than 8 days back: implausible");
        assert_eq!(health_since(&incident(BlueScreen, "blue_screen|abc", c)), None);
        assert_eq!(health_since(&incident(BlueScreen, "", c)), None);
    }

    fn row(seq: u64, ts: i64, offset: i64) -> TimelineRow {
        TimelineRow {
            offset_ms: offset,
            row: ActivityRow { seq, ts_utc_ms: ts, kind: "HealthSample".into(), pid: None, exe_name: None, detail: Detail::Unknown },
        }
    }

    #[test]
    fn under_the_cap_nothing_is_cut() {
        let (rows, truncated) = cap_rows(vec![row(1, 10, -5), row(2, 20, 5)], 2);
        assert_eq!((rows.len(), truncated), (2, false));
    }

    #[test]
    fn over_the_cap_the_rows_closest_to_the_incident_stay_in_time_order() {
        let all = vec![row(1, 100, -400), row(2, 200, -300), row(3, 300, -10), row(4, 400, 20), row(5, 500, 350), row(6, 600, 450)];
        let (rows, truncated) = cap_rows(all, 3);
        assert!(truncated);
        assert_eq!(
            rows.iter().map(|r| r.row.seq).collect::<Vec<_>>(),
            vec![2, 3, 4],
            "the nearest three (offsets 10, 20 and 300), oldest first"
        );
    }

    #[test]
    fn the_real_cap_is_five_thousand_rows() {
        assert_eq!(MAX_HEALTH_ROWS, 5_000);
        let many: Vec<TimelineRow> = (0..5_001u64).map(|i| row(i, i as i64, i as i64)).collect();
        let (rows, truncated) = cap_rows(many, MAX_HEALTH_ROWS);
        assert_eq!((rows.len(), truncated), (5_000, true));
    }
}
