//! Testes do Incident Engine: captura, preservação, anomalias e privacidade.

use std::cell::RefCell;
use std::rc::Rc;

use std::sync::{Arc, Mutex};

use bb_collector::{
    CollectError, ContextSource, CrashKind, CrashRecord, CrashSource, MetricsConfig, ProcessSample,
    ProcessSource, SystemSample,
};
use bb_core::{ExeName, GuardConfig, Observation, ProcessKey, RecorderState};
use bb_engine::{Engine, IncidentConfig};
use bb_recorder::{Recorder, RecorderConfig, StaticKey};
use bb_store::{CaptureState, IncidentKind, Severity, Store};

#[derive(Default)]
struct World {
    procs: Vec<ProcessSample>,
    foreground: Option<&'static str>,
}
type Shared = Rc<RefCell<World>>;
struct Procs(Shared);
struct Ctx(Shared);

/// Fonte de falhas que devolve TODOS os registros que tem, sem filtrar por horário: o pior caso.
/// O filtro "só o que foi registrado durante a gravação" tem que ser feito pelo engine.
#[derive(Clone, Default)]
struct FakeCrashes(Arc<Mutex<Vec<CrashRecord>>>);

impl CrashSource for FakeCrashes {
    fn poll(&mut self, _since_utc_ms: i64) -> Result<Vec<CrashRecord>, CollectError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

fn crash(ts: i64, name: &str, kind: CrashKind) -> CrashRecord {
    CrashRecord {
        ts_utc_ms: ts,
        kind,
        exe_name: ExeName::new(name).unwrap(),
        exception_code: (kind == CrashKind::Crash).then_some(0xc000_0005),
    }
}

impl ProcessSource for Procs {
    fn processes(&mut self) -> Result<Vec<ProcessSample>, CollectError> {
        Ok(self.0.borrow().procs.clone())
    }
    fn system(&mut self) -> Result<SystemSample, CollectError> {
        Ok(SystemSample { cpu_permille: 10, mem_used_kb: 1, mem_total_kb: 2 })
    }
}
impl ContextSource for Ctx {
    fn observe(&mut self) -> Observation {
        Observation {
            detector_ok: true,
            session_locked: false,
            foreground: self.0.borrow().foreground.map(|n| ExeName::new(n).unwrap()),
        }
    }
}

fn sample(pid: u32, name: &str, cpu: u64) -> ProcessSample {
    ProcessSample {
        key: ProcessKey { pid, start_time_ms: 1_000 },
        exe_name: ExeName::new(name).unwrap(),
        parent_pid: 1,
        cpu_time_100ns: cpu,
        working_set_kb: 10,
    }
}

struct Rig {
    engine: Engine<Procs, Ctx>,
    world: Shared,
    crashes: FakeCrashes,
    now: u64,
    _dir: tempfile::TempDir,
}

const TICK_MS: u64 = 500;

impl Rig {
    fn utc(&self) -> i64 {
        1_000_000 + self.now as i64
    }
    fn tick(&mut self) -> bb_engine::TickReport {
        self.now += TICK_MS;
        let utc = self.utc();
        self.engine.tick(self.now, utc).unwrap()
    }
    fn until_recording(&mut self) {
        for _ in 0..10 {
            if self.tick().state == RecorderState::Recording {
                return;
            }
        }
        panic!("never reached Recording");
    }
    /// Simula `busy` processos gastando 100% de um núcleo entre ciclos.
    fn burn(&mut self, pid: u32) {
        let mut w = self.world.borrow_mut();
        if let Some(p) = w.procs.iter_mut().find(|p| p.key.pid == pid) {
            p.cpu_time_100ns += TICK_MS * 10_000;
        }
    }
    fn store(&self) -> &Store {
        self.engine.store().unwrap()
    }
    fn total_events(&mut self) -> usize {
        self.engine.recorder_mut().seal().unwrap();
        let r = self.engine.recorder();
        r.list_segments().unwrap().iter().map(|s| r.read_segment(s.index).unwrap().len()).sum()
    }
}

fn rig() -> Rig {
    rig_at(tempfile::tempdir().unwrap())
}

/// Monta um engine sobre `dir`. Reusar o mesmo diretório simula reabrir o app depois de uma queda.
fn rig_at(dir: tempfile::TempDir) -> Rig {
    let world: Shared = Rc::new(RefCell::new(World {
        foreground: Some("synth-editor.exe"),
        procs: vec![sample(100, "synth-editor.exe", 0)],
        ..World::default()
    }));
    let rc = RecorderConfig { max_events_per_segment: 4, max_age: None, ..RecorderConfig::default() };
    let recorder = Recorder::open(dir.path().join("rec"), &StaticKey([5u8; 32]), rc).unwrap();
    let mut gc = GuardConfig::default();
    gc.stability_window_ms = 1_000;
    gc.excluded_apps.insert(ExeName::new("synth-excluded.exe").unwrap());
    let metrics = MetricsConfig { every_n_ticks: 1, min_cpu_permille: 0, min_working_set_kb: 0 };
    let mut engine = Engine::new(gc, metrics, recorder, Procs(world.clone()), Ctx(world.clone()), 1);
    let ic = IncidentConfig {
        pre_window_ms: 5_000,
        post_window_ms: 3_000,
        cpu_samples: 3,
        cooldown_ms: 1_000_000,
        ..IncidentConfig::default()
    };
    // Como no app: campos sensíveis do banco cifrados.
    engine.enable_incidents(Store::open_encrypted(dir.path().join("meta.db"), &[9u8; 32]).unwrap(), ic);
    let crashes = FakeCrashes::default();
    engine.set_crash_source(Box::new(crashes.clone()));
    Rig { engine, world, crashes, now: 0, _dir: dir }
}

#[test]
fn manual_capture_preserves_the_previous_window_immediately() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..6 {
        r.tick();
    }
    let id = r.engine.capture_manual(r.utc()).unwrap();
    let inc = r.store().get_incident(id).unwrap().unwrap();
    assert_eq!((inc.kind, inc.capture), (IncidentKind::Manual, CaptureState::Capturing));
    assert!(!inc.segments.is_empty(), "pre-window must be preserved right away");
    let segs = r.engine.recorder().list_segments().unwrap();
    for s in &inc.segments {
        assert!(segs.iter().find(|x| x.index == *s).unwrap().preserved);
    }
}

#[test]
fn capture_completes_after_the_post_window_and_extends_the_manifest() {
    let mut r = rig();
    r.until_recording();
    r.tick();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    let before = r.store().get_incident(id).unwrap().unwrap().segments;
    for _ in 0..10 {
        r.tick(); // passa da janela posterior de 3 s
    }
    let inc = r.store().get_incident(id).unwrap().unwrap();
    assert_eq!(inc.capture, CaptureState::Preserved);
    assert!(inc.segments.len() > before.len(), "post-window segments are added to the manifest");
    let segs = r.engine.recorder().list_segments().unwrap();
    for s in &inc.segments {
        assert!(segs.iter().find(|x| x.index == *s).unwrap().preserved);
    }
}

#[test]
fn post_window_completes_even_while_recording_is_blocked() {
    let mut r = rig();
    r.until_recording();
    r.tick();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    r.world.borrow_mut().foreground = Some("chrome.exe");
    for _ in 0..10 {
        assert_ne!(r.tick().state, RecorderState::Recording);
    }
    assert_eq!(r.store().get_incident(id).unwrap().unwrap().capture, CaptureState::Preserved);
}

#[test]
fn manual_capture_while_paused_collects_nothing_new() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..4 {
        r.tick();
    }
    r.engine.pause();
    let before = r.total_events();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    for _ in 0..10 {
        r.tick();
    }
    assert_eq!(r.total_events(), before, "no activity may be recorded during the pause");
    assert_eq!(r.store().get_incident(id).unwrap().unwrap().capture, CaptureState::Preserved);
}

#[test]
fn sustained_cpu_opens_an_incident_with_facts_only() {
    let mut r = rig();
    r.until_recording();
    let mut opened = 0;
    for _ in 0..8 {
        r.burn(100);
        opened += r.tick().incidents_opened;
    }
    assert_eq!(opened, 1, "cooldown must prevent repeats");
    let inc = r.store().list_incidents().unwrap().remove(0);
    assert_eq!(inc.kind, IncidentKind::CpuSustained);
    assert_eq!(inc.exe_name.as_deref(), Some("synth-editor.exe"));
    assert!(!inc.summary.contains("synth"));
    assert_eq!(inc.summary, "cpu_sustained|900|3", "a code with numeric parameters, no language");
    assert!(!inc.summary.to_lowercase().contains("caus"), "incidents state facts, not causes");
}

#[test]
fn the_incident_database_on_disk_never_holds_names_notes_or_rule_lists_in_clear() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..8 {
        r.burn(100);
        r.tick();
    }
    let inc = r.store().list_incidents().unwrap().remove(0);
    assert_eq!(inc.exe_name.as_deref(), Some("synth-editor.exe"), "readable through the API");
    r.store().add_note(inc.id, 1, "a private observation").unwrap();
    let mut s = quick_settings();
    s.excluded_apps = vec!["hidden-rule-app.exe".into()];
    r.engine.apply_settings(s, 1).unwrap();

    let bytes = std::fs::read(r._dir.path().join("meta.db")).unwrap();
    for secret in ["synth-editor.exe", "a private observation", "hidden-rule-app.exe", "chrome.exe"] {
        assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()), "found in clear on disk: {secret}");
    }
}

#[test]
fn excluded_apps_never_create_incidents() {
    let mut r = rig();
    r.world.borrow_mut().procs.push(sample(300, "synth-excluded.exe", 0));
    r.until_recording();
    for _ in 0..10 {
        r.burn(300);
        r.tick();
    }
    assert!(r.store().list_incidents().unwrap().is_empty());
}

#[test]
fn anomalies_during_a_pause_create_no_incident() {
    let mut r = rig();
    r.until_recording();
    r.engine.pause();
    for _ in 0..10 {
        r.burn(100);
        assert_eq!(r.tick().incidents_opened, 0);
    }
    assert!(r.store().list_incidents().unwrap().is_empty());
}

#[test]
fn deleting_an_incident_releases_only_segments_nobody_else_needs() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..6 {
        r.tick();
    }
    let a = r.engine.capture_manual(r.utc()).unwrap();
    let b = r.engine.capture_manual(r.utc()).unwrap();
    let shared = r.store().get_incident(a).unwrap().unwrap().segments;
    assert_eq!(shared, r.store().get_incident(b).unwrap().unwrap().segments);
    r.engine.delete_incident(a).unwrap();
    let segs = r.engine.recorder().list_segments().unwrap();
    assert!(shared.iter().all(|s| segs.iter().find(|x| x.index == *s).unwrap().preserved), "still referenced by b");
    r.engine.delete_incident(b).unwrap();
    let segs = r.engine.recorder().list_segments().unwrap();
    assert!(shared.iter().all(|s| !segs.iter().find(|x| x.index == *s).unwrap().preserved));
    assert!(r.store().list_incidents().unwrap().is_empty());
}

// ---- Windows Event Log: falhas e travamentos ----

#[test]
fn a_crash_logged_while_recording_is_persisted_and_opens_an_unexpected_exit_incident() {
    let mut r = rig();
    r.until_recording();
    let ts = r.utc() + 100;
    r.crashes.0.lock().unwrap().push(crash(ts, "synth-editor.exe", CrashKind::Crash));
    let mut opened = 0;
    for _ in 0..4 {
        opened += r.tick().incidents_opened; // a fonte devolve o mesmo registro em todo ciclo
    }
    assert_eq!(opened, 1, "duplicates from overlapping polls must be ignored");
    assert_eq!(lines_with(&mut r, "AppCrash"), 1);
    let inc = r.store().list_incidents().unwrap().remove(0);
    assert_eq!((inc.kind, inc.severity), (IncidentKind::UnexpectedExit, Severity::Critical));
    assert_eq!(inc.exe_name.as_deref(), Some("synth-editor.exe"));
    assert_eq!(inc.summary, "app_crash|3221225477");
    assert!(!inc.summary.contains("synth"), "summaries state facts, never names");
}

#[test]
fn a_hang_opens_an_app_hang_incident() {
    let mut r = rig();
    r.until_recording();
    let ts = r.utc() + 100;
    r.crashes.0.lock().unwrap().push(crash(ts, "synth-editor.exe", CrashKind::Hang));
    r.tick();
    r.tick();
    assert_eq!(r.store().list_incidents().unwrap()[0].kind, IncidentKind::AppHang);
}

// O que o Windows registrou ANTES da gravação começar nunca é reconstruído.
#[test]
fn a_crash_logged_before_recording_started_is_never_recorded() {
    let mut r = rig();
    r.crashes.0.lock().unwrap().push(crash(1_000_000 - 5_000, "synth-editor.exe", CrashKind::Crash));
    r.until_recording();
    for _ in 0..4 {
        r.tick();
    }
    assert_eq!(lines_with(&mut r, "AppCrash"), 0);
    assert!(r.store().list_incidents().unwrap().is_empty());
}

#[test]
fn a_crash_that_happened_during_a_pause_is_not_reconstructed_after_resume() {
    let mut r = rig();
    r.until_recording();
    r.engine.pause();
    r.tick();
    let during_pause = r.utc() + 50;
    r.crashes.0.lock().unwrap().push(crash(during_pause, "synth-editor.exe", CrashKind::Crash));
    for _ in 0..4 {
        r.tick();
    }
    r.engine.resume(r.now);
    r.until_recording();
    for _ in 0..4 {
        r.tick();
    }
    assert_eq!(lines_with(&mut r, "AppCrash"), 0, "a crash from the pause must never appear");
    assert!(r.store().list_incidents().unwrap().is_empty());
}

#[test]
fn a_crash_logged_during_a_privacy_block_is_not_reconstructed_either() {
    let mut r = rig();
    r.until_recording();
    r.world.borrow_mut().foreground = Some("chrome.exe");
    r.tick();
    let during_block = r.utc() + 50;
    r.crashes.0.lock().unwrap().push(crash(during_block, "synth-editor.exe", CrashKind::Crash));
    r.tick();
    r.tick();
    r.world.borrow_mut().foreground = Some("synth-editor.exe");
    r.until_recording();
    for _ in 0..4 {
        r.tick();
    }
    assert_eq!(lines_with(&mut r, "AppCrash"), 0);
}

#[test]
fn crashes_of_excluded_and_protected_apps_are_dropped() {
    let mut r = rig();
    let mut s = quick_settings();
    s.excluded_apps = vec!["synth-excluded.exe".into()];
    r.engine.apply_settings(s, 1).unwrap();
    r.until_recording();
    let ts = r.utc() + 100;
    {
        let mut c = r.crashes.0.lock().unwrap();
        c.push(crash(ts, "synth-excluded.exe", CrashKind::Crash));
        c.push(crash(ts, "Chrome.exe", CrashKind::Hang));
    }
    for _ in 0..4 {
        r.tick();
    }
    assert_eq!(lines_with(&mut r, "AppCrash") + lines_with(&mut r, "AppHang"), 0);
    assert!(r.store().list_incidents().unwrap().is_empty());
}

// ---- configurações e exclusão de dados ----

fn quick_settings() -> bb_engine::Settings {
    bb_engine::Settings { stability_window_ms: 1_000, ..bb_engine::Settings::default() }
}

fn lines_with(r: &mut Rig, needle: &str) -> usize {
    r.engine.recorder_mut().seal().unwrap();
    let rec = r.engine.recorder();
    rec.list_segments()
        .unwrap()
        .iter()
        .flat_map(|s| rec.read_segment(s.index).unwrap())
        .filter(|l| l.contains(needle))
        .count()
}

// ---- falha e recuperação (teste 15) ----

#[test]
fn a_crash_in_the_middle_of_an_incident_capture_is_recovered_on_restart() {
    let mut r1 = rig();
    r1.until_recording();
    for _ in 0..6 {
        r1.tick();
    }
    let id = r1.engine.capture_manual(r1.utc()).unwrap();
    r1.tick(); // ainda dentro da janela posterior: a captura está pendente
    let (now, created) = (r1.now, r1.store().get_incident(id).unwrap().unwrap().created_utc_ms);
    assert_eq!(r1.store().get_incident(id).unwrap().unwrap().capture, CaptureState::Capturing);

    // O app cai: sem seal, sem shutdown, sem Drop do engine.
    let Rig { engine, _dir, .. } = r1;
    std::mem::forget(engine);

    // Reabre sobre os mesmos dados.
    let mut r2 = rig_at(_dir);
    r2.now = now;
    let report = r2.engine.recorder().recovery_report().expect("an orphan journal must be recovered");
    assert!(report.recovered_events > 0, "events in the orphan journal are sealed, not lost");

    // O primeiro ciclo depois da janela conclui a captura que estava pendente.
    for _ in 0..12 {
        r2.tick();
    }
    let inc = r2.store().get_incident(id).unwrap().unwrap();
    assert_eq!(inc.capture, CaptureState::Preserved, "the interrupted capture is completed");
    assert_eq!(inc.created_utc_ms, created);
    let segs = r2.engine.recorder().list_segments().unwrap();
    for s in &inc.segments {
        assert!(segs.iter().find(|x| x.index == *s).is_some_and(|x| x.preserved), "evidence {s} must be preserved");
    }
    assert!(r2.engine.recorder().verify().is_ok(), "the chain is intact after the crash");
    assert_eq!(bb_engine::Settings::load(r2.store()), bb_engine::Settings::default());
}

// ---- exportação com nova filtragem (teste 11) ----

fn export(r: &mut Rig, id: i64) -> (bb_engine::ExportResult, String) {
    let out = r._dir.path().join("exports");
    let res = r.engine.export_incident(id, &out, r.now, r.utc()).unwrap();
    let text = std::fs::read_to_string(&res.path).unwrap();
    (res, text)
}

fn rig_with_two_apps() -> Rig {
    let mut r = rig();
    r.world.borrow_mut().procs.push(sample(200, "synth-second.exe", 0));
    r.until_recording();
    for _ in 0..4 {
        r.tick();
    }
    r
}

#[test]
fn an_export_is_a_generated_file_with_only_allowlisted_fields_and_no_notes() {
    let mut r = rig_with_two_apps();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    r.store().add_note(id, 1, "a private observation").unwrap();
    let (res, text) = export(&mut r, id);
    assert!(res.path.file_name().unwrap().to_string_lossy().starts_with(&format!("incident-{id}-")));
    assert!(res.path.starts_with(r._dir.path().join("exports")));
    assert!(res.events > 0);
    assert!(!text.contains("a private observation"), "notes never leave the store");

    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["notesIncluded"], false);
    let allowed = ["offsetMs", "seq", "tsUtcMs", "kind", "pid", "exeName", "detail"];
    for e in v["events"].as_array().unwrap() {
        for k in e.as_object().unwrap().keys() {
            assert!(allowed.contains(&k.as_str()), "unexpected field in export: {k}");
        }
    }
}

#[test]
fn an_export_reapplies_the_current_exclusion_rules() {
    let mut r = rig_with_two_apps();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    let (_, before) = export(&mut r, id);
    assert!(before.contains("synth-second.exe"), "control: it was recorded and is allowed at first");

    let mut s = quick_settings();
    s.excluded_apps = vec!["synth-second.exe".into()];
    r.engine.apply_settings(s, 1).unwrap();
    let (res, after) = export(&mut r, id);
    assert!(!after.contains("synth-second.exe"), "excluded after recording: must not be exported");
    assert!(!after.contains("\"pid\": 200"), "not even by pid");
    assert!(res.dropped > 0);
    assert!(after.contains("synth-editor.exe"), "other apps are still exported");
}

#[test]
fn an_export_drops_apps_that_became_protected_after_recording() {
    let mut r = rig_with_two_apps();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    let mut s = quick_settings();
    s.protected_apps.push("synth-second.exe".into());
    r.engine.apply_settings(s, 1).unwrap();
    let (res, text) = export(&mut r, id);
    assert!(!text.contains("synth-second.exe"));
    assert!(res.dropped > 0);
}

#[test]
fn an_export_fails_closed_for_events_whose_app_cannot_be_identified() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..40 {
        r.tick(); // vários segmentos: o início do processo fica num segmento antigo
    }
    let id = r.engine.capture_manual(r.utc()).unwrap();
    let (res, text) = export(&mut r, id);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let has_metrics = v["events"].as_array().unwrap().iter().any(|e| e["kind"] == "ProcessMetrics");
    assert!(!has_metrics, "metrics whose ProcessStarted is not in the evidence must be dropped");
    assert!(res.dropped > 0);
}

#[test]
fn the_incident_process_name_is_redacted_when_its_app_is_now_excluded() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..8 {
        r.burn(100);
        r.tick();
    }
    let id = r.store().list_incidents().unwrap().remove(0).id;
    let (_, before) = export(&mut r, id);
    assert!(before.contains("\"exeName\": \"synth-editor.exe\""));
    let mut s = quick_settings();
    s.excluded_apps = vec!["synth-editor.exe".into()];
    r.engine.apply_settings(s, 1).unwrap();
    let (_, after) = export(&mut r, id);
    assert!(!after.contains("synth-editor.exe"));
}

#[test]
fn exporting_an_unknown_incident_fails() {
    let mut r = rig();
    assert!(r.engine.export_incident(999, &r._dir.path().join("exports"), 0, 0).is_err());
}

// ---- autorizações de teste no engine ----

#[test]
fn engine_errors_reach_the_interface_as_language_neutral_codes() {
    let mut r = rig();
    let code = |res: Result<(), bb_engine::EngineError>| res.unwrap_err().code();
    assert_eq!(code(r.engine.authorize_app("synth-editor.exe", 10, true, true, r.now, 1)), "auth.not_protected");
    assert_eq!(code(r.engine.authorize_app("chrome.exe", 0, true, true, r.now, 1)), "auth.bad_duration");
    assert_eq!(code(r.engine.authorize_app("chrome.exe", 10, false, false, r.now, 1)), "auth.no_source");
    assert_eq!(code(r.engine.authorize_app("bad/name", 10, true, true, r.now, 1)), "auth.bad_name");
    let mut s = quick_settings();
    s.stability_window_ms = 1;
    assert_eq!(code(r.engine.apply_settings(s, 1)), "settings.stability_range");
    let out = r._dir.path().join("exports");
    assert_eq!(r.engine.export_incident(999, &out, 0, 0).unwrap_err().code(), "export.not_found");
    // um erro interno nunca vaza detalhes (podem conter caminhos)
    let internal = bb_engine::EngineError::Store(bb_store::StoreError("C:\\secret\\path failed".into()));
    assert_eq!(internal.code(), "store.error");
}

#[test]
fn engine_authorizations_are_validated_listed_revoked_and_logged_without_names() {
    let mut r = rig();
    r.world.borrow_mut().procs.push(sample(500, "chrome.exe", 0));
    r.until_recording();
    assert!(r.engine.authorize_app("synth-editor.exe", 10, true, true, r.now, 1).is_err(), "not a protected app");
    assert!(r.engine.authorize_app("chrome.exe", 0, true, true, r.now, 1).is_err(), "bad duration");
    assert!(r.engine.authorize_app("chrome.exe", 10, false, false, r.now, 1).is_err(), "no source");
    assert!(r.engine.authorizations(r.now).is_empty());

    r.engine.authorize_app("Chrome.EXE", 10, true, false, r.now, 7).unwrap();
    let list = r.engine.authorizations(r.now);
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].exe.as_str(), list[0].allow_metrics, list[0].allow_crashes), ("chrome.exe", true, false));
    assert!(list[0].remaining_ms > 9 * 60_000 && list[0].remaining_ms <= 10 * 60_000);

    assert!(r.engine.revoke_authorization("chrome.exe", 8).unwrap());
    assert!(!r.engine.revoke_authorization("chrome.exe", 9).unwrap(), "already revoked");
    assert!(r.engine.authorizations(r.now).is_empty());

    let hist = r.store().config_history(10).unwrap();
    assert!(hist.iter().any(|h| h.key == "authorizations" && h.change == "added" && h.at_utc_ms == 7));
    assert!(hist.iter().any(|h| h.key == "authorizations" && h.change == "removed"));
    assert!(!format!("{hist:?}").to_lowercase().contains("chrome"), "history must not name the app");
}

#[test]
fn a_test_authorization_records_only_the_authorized_browser_while_it_is_in_the_foreground() {
    let mut r = rig();
    {
        let mut w = r.world.borrow_mut();
        w.procs.push(sample(500, "chrome.exe", 0));
    }
    r.until_recording();
    r.tick();
    r.world.borrow_mut().foreground = Some("chrome.exe");
    assert_eq!(r.tick().state, RecorderState::PrivacyBlocked, "no authorization: blocked");

    r.engine.authorize_app("chrome.exe", 30, true, true, r.now, 1).unwrap();
    r.until_recording();
    let editor_events_before = lines_with(&mut r, "\"pid\":100");
    for _ in 0..6 {
        r.tick();
    }
    assert!(lines_with(&mut r, "\"pid\":500") > 0, "the authorized browser's technical events are recorded");
    assert_eq!(
        lines_with(&mut r, "\"pid\":100"),
        editor_events_before,
        "nothing about other apps while in restricted mode"
    );
    assert_eq!(lines_with(&mut r, "SystemMetrics") > 0, true);
    let sys_before = lines_with(&mut r, "SystemMetrics");
    for _ in 0..4 {
        r.tick();
    }
    assert_eq!(lines_with(&mut r, "SystemMetrics"), sys_before, "no system metrics in restricted mode");
}

#[test]
fn an_exclusion_added_mid_run_stops_events_of_already_tracked_processes() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..3 {
        r.tick();
    }
    let before = lines_with(&mut r, "\"pid\":100");
    assert!(before > 0);

    let mut s = quick_settings();
    s.excluded_apps = vec!["synth-editor.exe".into()];
    let utc = r.utc();
    r.engine.apply_settings(s, utc).unwrap();
    for _ in 0..10 {
        r.tick();
    }
    assert_eq!(lines_with(&mut r, "\"pid\":100"), before, "no new events of the now-excluded app");
}

#[test]
fn a_newly_protected_foreground_app_blocks_recording_immediately() {
    let mut r = rig();
    r.until_recording();
    let mut s = quick_settings();
    s.protected_apps.push("synth-editor.exe".into());
    let utc = r.utc();
    r.engine.apply_settings(s, utc).unwrap();
    assert_eq!(r.tick().state, RecorderState::PrivacyBlocked);
}

#[test]
fn settings_history_records_the_kind_of_change_and_never_the_value() {
    let mut r = rig();
    let mut s = quick_settings();
    s.excluded_apps = vec!["very-secret-app.exe".into()];
    r.engine.apply_settings(s, 42).unwrap();
    let hist = r.store().config_history(10).unwrap();
    assert!(hist.iter().any(|h| h.key == "excluded_apps" && h.change == "added" && h.at_utc_ms == 42));
    let dump = format!("{hist:?}");
    assert!(!dump.contains("very-secret-app"), "config history must not contain values");
}

#[test]
fn invalid_settings_are_rejected_and_change_nothing() {
    let mut r = rig();
    let before = r.engine.settings().clone();
    let mut bad = quick_settings();
    bad.excluded_apps = vec!["C:\\Windows\\evil.exe".into()];
    assert!(r.engine.apply_settings(bad, 1).is_err());
    let mut bad = quick_settings();
    bad.stability_window_ms = 5;
    assert!(r.engine.apply_settings(bad, 1).is_err());
    assert_eq!(r.engine.settings(), &before);
    assert!(r.store().config_history(10).unwrap().is_empty());
}

#[test]
fn saved_settings_can_be_loaded_back() {
    let mut r = rig();
    let mut s = quick_settings();
    s.excluded_apps = vec!["foo.exe".into()];
    s.retention_max_hours = 48;
    r.engine.apply_settings(s.clone(), 1).unwrap();
    assert_eq!(bb_engine::Settings::load(r.store()), s.normalized());
}

#[test]
fn deleting_activity_keeps_preserved_evidence_unless_asked() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..6 {
        r.tick();
    }
    let id = r.engine.capture_manual(r.utc()).unwrap();
    for _ in 0..12 {
        r.tick();
    }
    let preserved = r.store().get_incident(id).unwrap().unwrap().segments;
    assert!(!preserved.is_empty());

    r.engine.delete_activity(false).unwrap();
    let left: Vec<u64> = r.engine.recorder().list_segments().unwrap().iter().map(|s| s.index).collect();
    assert_eq!(left, preserved, "only the evidence remains");
    assert!(r.engine.recorder().verify().is_ok(), "deleted segments leave a verifiable gap");
    assert!(r.store().get_incident(id).unwrap().is_some());

    r.engine.delete_activity(true).unwrap();
    assert!(r.engine.recorder().list_segments().unwrap().is_empty());
    assert!(r.store().list_incidents().unwrap().is_empty(), "incidents without evidence are removed");
    assert!(r.engine.recorder().verify().is_ok());
}

#[test]
fn shutdown_finalizes_pending_captures() {
    let mut r = rig();
    r.until_recording();
    r.tick();
    let id = r.engine.capture_manual(r.utc()).unwrap();
    r.engine.shutdown().unwrap();
    assert_eq!(r.store().get_incident(id).unwrap().unwrap().capture, CaptureState::Preserved);
}

// ---- exclusão por tipo de evento (regras parciais) ----

use bb_core::ExclusionSet as X;
use bb_engine::PartialExclusion;

fn rule(exe: &str, set: X) -> PartialExclusion {
    PartialExclusion { exe: exe.into(), excluded: set }
}

fn export_json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

/// Tipos de evento exportados de um programa (pelo nome que a exportação mostra).
fn exported_kinds(v: &serde_json::Value, exe: &str) -> std::collections::BTreeSet<String> {
    v["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["exeName"] == exe)
        .map(|e| e["kind"].as_str().unwrap().to_owned())
        .collect()
}

fn count_lines_with_both(r: &mut Rig, a: &str, b: &str) -> usize {
    r.engine.recorder_mut().seal().unwrap();
    let rec = r.engine.recorder();
    rec.list_segments()
        .unwrap()
        .iter()
        .flat_map(|s| rec.read_segment(s.index).unwrap())
        .filter(|l| l.contains(a) && l.contains(b))
        .count()
}

#[test]
fn an_export_reapplies_exclusions_per_event_kind() {
    let mut r = rig_with_two_apps();
    let ts = r.utc() + 100;
    r.crashes.0.lock().unwrap().push(crash(ts, "synth-second.exe", CrashKind::Crash));
    for _ in 0..3 {
        r.tick();
    }
    let id = r.engine.capture_manual(r.utc()).unwrap();
    let second = "synth-second.exe";
    let editor = "synth-editor.exe";

    let (_, text) = export(&mut r, id);
    let base_second = exported_kinds(&export_json(&text), second);
    let base_editor = exported_kinds(&export_json(&text), editor);
    for k in ["ProcessStarted", "ProcessMetrics", "AppCrash"] {
        assert!(base_second.contains(k), "control: {k} was recorded and is exported at first");
    }

    let cases: [(X, &[&str]); 3] = [
        (X::new(false, true, false), &["ProcessMetrics"]),
        (X::new(true, false, false), &["ProcessStarted", "ProcessExited", "ProcessMetrics"]),
        (X::new(false, false, true), &["AppCrash", "AppHang"]),
    ];
    for (set, gone) in cases {
        let mut s = quick_settings();
        s.partial_exclusions = vec![rule(second, set)];
        let utc = r.utc();
        r.engine.apply_settings(s, utc).unwrap();
        let (res, text) = export(&mut r, id);
        let v = export_json(&text);
        let got = exported_kinds(&v, second);
        let want: std::collections::BTreeSet<String> = base_second.iter().filter(|k| !gone.contains(&k.as_str())).cloned().collect();
        assert_eq!(got, want, "kinds exported for {set:?}");
        assert_eq!(exported_kinds(&v, editor), base_editor, "other programs are untouched by {set:?}");
        assert!(res.dropped > 0 || got == base_second);
    }
}

#[test]
fn the_incident_process_name_is_hidden_when_its_app_has_any_partial_exclusion() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..8 {
        r.burn(100);
        r.tick();
    }
    let id = r.store().list_incidents().unwrap().remove(0).id;
    let (_, before) = export(&mut r, id);
    assert_eq!(export_json(&before)["incident"]["exeName"], "synth-editor.exe", "control");

    let mut s = quick_settings();
    s.partial_exclusions = vec![rule("synth-editor.exe", X::new(false, false, true))]; // só falhas
    let utc = r.utc();
    r.engine.apply_settings(s, utc).unwrap();
    let (_, after) = export(&mut r, id);
    let incident = &export_json(&after)["incident"];
    assert!(incident.get("exeName").is_none_or(|n| n.is_null()), "name hidden: {incident}");
}

#[test]
fn a_partial_exclusion_stops_only_that_kind_of_events_in_a_running_engine() {
    let mut r = rig();
    r.until_recording();
    for _ in 0..3 {
        r.tick();
    }
    let metrics_before = count_lines_with_both(&mut r, "ProcessMetrics", "\"pid\":100");
    assert!(metrics_before > 0);

    // Exclui só CPU e memória do app.
    let mut s = quick_settings();
    s.partial_exclusions = vec![rule("synth-editor.exe", X::new(false, true, false))];
    let utc = r.utc();
    r.engine.apply_settings(s, utc).unwrap();
    for _ in 0..6 {
        r.tick();
    }
    assert_eq!(count_lines_with_both(&mut r, "ProcessMetrics", "\"pid\":100"), metrics_before, "no new metrics");
    // O fim do processo continua sendo gravado.
    let exits_before = count_lines_with_both(&mut r, "ProcessExited", "\"pid\":100");
    r.world.borrow_mut().procs.retain(|p| p.key.pid != 100);
    for _ in 0..3 {
        r.tick();
    }
    assert_eq!(count_lines_with_both(&mut r, "ProcessExited", "\"pid\":100"), exits_before + 1, "the end is recorded");
}

#[test]
fn partial_rules_are_normalized_validated_and_saved_encrypted() {
    let mut r = rig();
    let mut s = quick_settings();
    s.excluded_apps = vec!["already-full.exe".into()];
    s.partial_exclusions = vec![
        rule(" Partial-Secret-App.EXE ", X::new(false, true, false)),
        rule("partial-secret-app.exe", X::new(false, false, true)), // duplicado: união
        rule("promoted.exe", X::ALL),                              // tudo -> exclusão total
        rule("already-full.exe", X::new(false, true, false)),      // já excluído por inteiro: some
        rule("empty.exe", X::NONE),                                // sem tipos: some
    ];
    r.engine.apply_settings(s, 1).unwrap();
    let saved = r.engine.settings().clone();
    assert_eq!(saved.excluded_apps, vec!["already-full.exe", "promoted.exe"]);
    assert_eq!(saved.partial_exclusions, vec![rule("partial-secret-app.exe", X::new(false, true, true))]);
    assert_eq!(bb_engine::Settings::load(r.store()), saved, "round trip through the store");

    let bytes = std::fs::read(r._dir.path().join("meta.db")).unwrap();
    for secret in ["partial-secret-app", "promoted.exe", "already-full.exe"] {
        assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()), "found in clear on disk: {secret}");
    }
}

#[test]
fn invalid_partial_rules_are_rejected_and_change_nothing() {
    let mut r = rig();
    let before = r.engine.settings().clone();
    let mut bad = quick_settings();
    bad.partial_exclusions = vec![rule("C:\\Windows\\evil.exe", X::new(false, true, false))];
    assert_eq!(r.engine.apply_settings(bad, 1).unwrap_err().code(), "settings.bad_excluded_name");
    let mut many = quick_settings();
    many.partial_exclusions = (0..201).map(|i| rule(&format!("a{i}.exe"), X::new(false, true, false))).collect();
    assert_eq!(r.engine.apply_settings(many, 1).unwrap_err().code(), "settings.too_many_excluded");
    assert_eq!(r.engine.settings(), &before);
    assert!(r.store().config_history(10).unwrap().is_empty());
}

#[test]
fn databases_from_before_partial_rules_load_exactly_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_encrypted(dir.path().join("meta.db"), &[9u8; 32]).unwrap();
    store.set_setting("excluded_apps", "old-app.exe\nother-old.exe").unwrap();
    store.set_setting("auto_start", "true").unwrap();
    let s = bb_engine::Settings::load(&store);
    assert_eq!(s.excluded_apps, vec!["old-app.exe", "other-old.exe"], "existing exclusions keep meaning 'everything'");
    assert!(s.partial_exclusions.is_empty());
    assert!(s.auto_start);
}

#[test]
fn an_unreadable_partial_rule_in_the_database_fails_closed_to_full_exclusion() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_encrypted(dir.path().join("meta.db"), &[9u8; 32]).unwrap();
    store
        .set_setting("partial_exclusions", "good.exe|metrics\nbogus-kind.exe|nonsense\nno-kinds.exe|\nno-separator.exe\nmixed.exe|crashes,nonsense")
        .unwrap();
    let s = bb_engine::Settings::load(&store);
    assert_eq!(s.partial_exclusions, vec![rule("good.exe", X::new(false, true, false))]);
    for exe in ["bogus-kind.exe", "no-kinds.exe", "no-separator.exe", "mixed.exe"] {
        assert!(s.excluded_apps.iter().any(|a| a == exe), "{exe} must become a FULL exclusion, never be ignored");
    }
}

#[test]
fn settings_history_for_partial_rules_records_only_the_key_and_the_kind_of_change() {
    let mut r = rig();
    let mut s = quick_settings();
    s.partial_exclusions = vec![rule("partial-secret-app.exe", X::new(false, true, false))];
    r.engine.apply_settings(s.clone(), 10).unwrap();
    s.partial_exclusions = vec![rule("partial-secret-app.exe", X::new(false, true, true))];
    r.engine.apply_settings(s.clone(), 11).unwrap();
    s.partial_exclusions = vec![];
    r.engine.apply_settings(s, 12).unwrap();

    let hist = r.store().config_history(10).unwrap();
    let changes: Vec<(i64, &str)> = hist.iter().filter(|h| h.key == "partial_exclusions").map(|h| (h.at_utc_ms, h.change.as_str())).collect();
    assert!(changes.contains(&(10, "added")) && changes.contains(&(11, "changed")) && changes.contains(&(12, "removed")), "{changes:?}");
    let dump = format!("{hist:?}");
    for leak in ["partial-secret-app", "lifecycle", "metrics", "crashes"] {
        assert!(!dump.contains(leak), "config history must not contain values: {leak}");
    }
}
