//! Testes das consultas do dashboard. Dados sintéticos, recorder real.

use std::collections::BTreeSet;
use std::fs;

use bb_core::{EventKind, ExeName, GuardConfig, Observation, PrivacyGuard, ProcessKey};
use bb_query::{activity, incident_detail, overview, processes, ActivityFilter, Detail};
use bb_recorder::{Recorder, RecorderConfig, StaticKey};
use bb_store::{IncidentKind, NewIncident, Severity, Store};

fn key(pid: u32) -> ProcessKey {
    ProcessKey { pid, start_time_ms: 100 }
}

fn exe(n: &str) -> ExeName {
    ExeName::new(n).unwrap()
}

/// 6 eventos: com 4 por segmento, 4 ficam selados (segmento 0) e 2 no journal ativo.
fn scenario() -> (Recorder, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = RecorderConfig { max_events_per_segment: 4, max_age: None, ..RecorderConfig::default() };
    let mut rec = Recorder::open(dir.path(), &StaticKey([1u8; 32]), cfg).unwrap();

    let mut g = PrivacyGuard::new(GuardConfig::default());
    let obs = Observation { detector_ok: true, session_locked: false, foreground: Some(exe("synth-editor.exe")) };
    g.observe(0, obs.clone());
    g.observe(6_000, obs);
    let t = 6_000;

    let evs: Vec<(i64, EventKind)> = vec![
        (1_000, EventKind::ProcessStarted { key: key(1), exe_name: exe("synth-editor.exe"), parent_pid: 0 }),
        (2_000, EventKind::ProcessStarted { key: key(2), exe_name: exe("synth-worker.exe"), parent_pid: 1 }),
        (3_000, EventKind::ProcessMetrics { key: key(1), cpu_permille: 250, working_set_kb: 20_480 }),
        (3_500, EventKind::ProcessMetrics { key: key(2), cpu_permille: 900, working_set_kb: 102_400 }),
        (4_000, EventKind::SystemMetrics { cpu_permille: 300, mem_used_kb: 4_096_000, mem_total_kb: 8_192_000 }),
        (5_000, EventKind::ProcessExited { key: key(2), exit_code: None }),
    ];
    for (ts, kind) in evs {
        let ev = g.admit(t, ts, kind).expect("guard admits synthetic event");
        rec.append(&ev).unwrap();
    }
    (rec, dir)
}

fn f(limit: usize) -> ActivityFilter {
    ActivityFilter { limit, ..ActivityFilter::default() }
}

#[test]
fn activity_is_newest_first_across_sealed_segments_and_the_active_journal() {
    let (rec, _d) = scenario();
    assert_eq!(rec.list_segments().unwrap().len(), 1, "4 events sealed");
    let rows = activity(&rec, &f(100));
    assert_eq!(rows.len(), 6, "sealed + journal");
    let ts: Vec<i64> = rows.iter().map(|r| r.ts_utc_ms).collect();
    assert_eq!(ts, vec![5_000, 4_000, 3_500, 3_000, 2_000, 1_000]);
}

#[test]
fn sealing_the_journal_does_not_change_or_duplicate_results() {
    let (mut rec, _d) = scenario();
    let before = activity(&rec, &f(100));
    rec.seal().unwrap();
    assert_eq!(activity(&rec, &f(100)), before);
}

#[test]
fn filters_by_kind_time_text_and_limit() {
    let (rec, _d) = scenario();
    let only_metrics = ActivityFilter { kinds: vec!["ProcessMetrics".into()], limit: 100, ..Default::default() };
    assert_eq!(activity(&rec, &only_metrics).len(), 2);

    let window = ActivityFilter { from_utc_ms: Some(2_500), to_utc_ms: Some(4_500), limit: 100, ..Default::default() };
    assert_eq!(activity(&rec, &window).len(), 3);

    assert_eq!(activity(&rec, &f(2)).len(), 2);

    // texto: procura no nome do executável, inclusive em eventos que só têm o pid (métricas/saída)
    let worker = ActivityFilter { text: "WORKER".into(), limit: 100, ..Default::default() };
    let rows = activity(&rec, &worker);
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, vec!["ProcessExited", "ProcessMetrics", "ProcessStarted"]);
    assert!(rows.iter().all(|r| r.exe_name.as_deref() == Some("synth-worker.exe")));
}

#[test]
fn rows_expose_only_allowlisted_fields() {
    let (rec, _d) = scenario();
    let allowed: BTreeSet<&str> = ["seq", "tsUtcMs", "kind", "pid", "exeName", "detail"].into_iter().collect();
    for r in activity(&rec, &f(100)) {
        let v = serde_json::to_value(&r).unwrap();
        for k in v.as_object().unwrap().keys() {
            assert!(allowed.contains(k.as_str()), "unexpected field in row: {k}");
        }
    }
}

// O detalhe é estruturado (código + números), sem texto de idioma nenhum: quem o exibe decide o idioma.
#[test]
fn details_are_language_neutral_codes_and_numbers() {
    let (rec, _d) = scenario();
    let rows = activity(&rec, &f(100));
    let metric = rows.iter().find(|r| r.kind == "ProcessMetrics" && r.pid == Some(1)).unwrap();
    assert_eq!(metric.detail, Detail::ProcessMetrics { cpu_permille: 250, working_set_kb: 20_480 });
    let sys = rows.iter().find(|r| r.kind == "SystemMetrics").unwrap();
    assert_eq!(sys.detail, Detail::SystemMetrics { cpu_permille: 300, mem_used_kb: 4_096_000, mem_total_kb: 8_192_000 });
    let exited = rows.iter().find(|r| r.kind == "ProcessExited").unwrap();
    assert_eq!(exited.detail, Detail::ProcessExited { exit_code: None });
    let started = rows.iter().find(|r| r.kind == "ProcessStarted" && r.pid == Some(2)).unwrap();
    assert_eq!(started.detail, Detail::ProcessStarted { parent_pid: 1 });
}

#[test]
fn serialized_details_hold_only_a_code_and_numeric_fields() {
    let (rec, _d) = scenario();
    let allowed: BTreeSet<&str> =
        ["code", "parentPid", "exitCode", "cpuPermille", "workingSetKb", "memUsedKb", "memTotalKb", "exceptionCode", "marker"]
            .into_iter()
            .collect();
    for r in activity(&rec, &f(100)) {
        let v = serde_json::to_value(&r.detail).unwrap();
        for (k, val) in v.as_object().unwrap() {
            assert!(allowed.contains(k.as_str()), "unexpected detail field: {k}");
            if k != "code" {
                assert!(val.is_number() || val.is_null(), "detail field {k} must be numeric, got {val}");
            }
        }
    }
}

#[test]
fn processes_show_running_first_then_ended() {
    let (rec, _d) = scenario();
    let ps = processes(&rec, 50);
    assert_eq!(ps.len(), 2);
    assert_eq!((ps[0].pid, ps[0].running), (1, true));
    assert_eq!(ps[0].cpu_permille, Some(250));
    assert_eq!((ps[1].pid, ps[1].running, ps[1].ended_utc_ms), (2, false, Some(5_000)));
    assert_eq!(ps[1].working_set_kb, Some(102_400));
}

#[test]
fn overview_summarizes_the_last_hour_and_storage() {
    let (rec, _d) = scenario();
    let o = overview(&rec, None, 6_000);
    assert_eq!((o.events_last_hour, o.starts_last_hour, o.exits_last_hour), (6, 2, 1));
    assert_eq!(o.latest_system.unwrap().cpu_permille, 300);
    assert!(o.storage_bytes > 0);
    assert_eq!(o.sealed_segments, 1);
    // fora da janela de 1 h nada conta
    let old = overview(&rec, None, 10_000_000);
    assert_eq!(old.events_last_hour, 0);
}

#[test]
fn incident_detail_shows_only_evidence_segments_with_relative_offsets() {
    let (rec, _d) = scenario();
    let store = Store::open_in_memory().unwrap();
    let id = store
        .create_incident(&NewIncident {
            kind: IncidentKind::Manual,
            severity: Severity::Info,
            created_utc_ms: 3_000,
            exe_name: None,
            summary: "Manual capture".into(),
            post_until_utc_ms: 4_000,
        })
        .unwrap();
    store.set_segments(id, &[0]).unwrap();
    store.add_note(id, 9, "my own note").unwrap();

    let d = incident_detail(&rec, &store, id).unwrap();
    assert_eq!(d.segments.len(), 1);
    assert_eq!(d.timeline.len(), 4, "only the 4 events of segment 0, not the 2 in the journal");
    assert_eq!(d.timeline[0].offset_ms, 1_000 - 3_000);
    assert!(d.timeline.windows(2).all(|w| w[0].row.ts_utc_ms <= w[1].row.ts_utc_ms), "chronological");
    assert_eq!(d.notes.len(), 1);
    assert!(incident_detail(&rec, &store, 999).is_none());
}

#[test]
fn an_unreadable_segment_is_skipped_without_failing_the_dashboard() {
    let (rec, dir) = scenario();
    let p = dir.path().join("seg-0000000000.bbseg");
    let mut b = fs::read(&p).unwrap();
    let last = b.len() - 1;
    b[last] ^= 1;
    fs::write(&p, b).unwrap();
    let rows = activity(&rec, &f(100));
    assert_eq!(rows.len(), 2, "only the journal events remain readable");
    assert!(rec.verify().is_err(), "the integrity check is what reports the corruption");
}

#[test]
fn a_health_event_row_exposes_only_category_id_and_number() {
    let dir = tempfile::tempdir().unwrap();
    let mut rec = Recorder::open(dir.path(), &StaticKey([1u8; 32]), RecorderConfig::default()).unwrap();
    let mut g = PrivacyGuard::new(GuardConfig::default());
    g.observe(0, Observation { detector_ok: true, session_locked: false, foreground: Some(exe("synth-editor.exe")) });
    let kind = EventKind::HealthEvent { category: bb_core::HealthCategory::BugCheck, event_id: 1001, code: Some(0xd1) };
    let ev = g.admit_health(1, 7_000, kind).expect("health door admits it");
    rec.append(&ev).unwrap();

    let rows = activity(&rec, &f(10));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "HealthEvent");
    assert_eq!(rows[0].exe_name, None);
    assert_eq!(rows[0].detail, Detail::HealthEvent { category: "BugCheck".into(), event_id: 1001, value: Some(0xd1) });
    let json = serde_json::to_string(&rows[0]).unwrap();
    assert!(json.contains("\"code\":\"healthEvent\"") && json.contains("\"eventId\":1001"), "{json}");
}
