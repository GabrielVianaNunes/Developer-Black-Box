//! Testes de privacidade da cadeia completa: collector → Guard → recorder.
//! Fontes falsas e dados sintéticos.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use bb_collector::{
    CollectError, ContextSource, MetricsConfig, ProcessSample, ProcessSource, SystemSample,
};
use bb_core::{ExeName, GuardConfig, Observation, ProcessKey, ReasonCode, RecorderState};
use bb_engine::Engine;
use bb_recorder::{Recorder, RecorderConfig, StaticKey};

#[derive(Default)]
struct World {
    procs: Vec<ProcessSample>,
    foreground: Option<&'static str>,
    locked: bool,
    proc_calls: u32,
    ctx_calls: u32,
    fail_processes: bool,
}

type Shared = Rc<RefCell<World>>;

struct FakeProcs(Shared);
struct FakeCtx(Shared);

impl ProcessSource for FakeProcs {
    fn processes(&mut self) -> Result<Vec<ProcessSample>, CollectError> {
        let mut w = self.0.borrow_mut();
        w.proc_calls += 1;
        if w.fail_processes {
            return Err(CollectError("synthetic failure".into()));
        }
        Ok(w.procs.clone())
    }
    fn system(&mut self) -> Result<SystemSample, CollectError> {
        Ok(SystemSample { cpu_permille: 10, mem_used_kb: 1, mem_total_kb: 2 })
    }
}

impl ContextSource for FakeCtx {
    fn observe(&mut self) -> Observation {
        let mut w = self.0.borrow_mut();
        w.ctx_calls += 1;
        Observation {
            detector_ok: true,
            session_locked: w.locked,
            foreground: w.foreground.map(|n| ExeName::new(n).unwrap()),
        }
    }
}

fn proc(pid: u32, start: i64, name: &str) -> ProcessSample {
    ProcessSample {
        key: ProcessKey { pid, start_time_ms: start },
        exe_name: ExeName::new(name).unwrap(),
        parent_pid: 1,
        cpu_time_100ns: 0,
        working_set_kb: 10,
    }
}

struct Rig {
    engine: Engine<FakeProcs, FakeCtx>,
    world: Shared,
    _dir: tempfile::TempDir,
}

fn rig(guard: GuardConfig) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let world: Shared = Rc::new(RefCell::new(World {
        foreground: Some("synth-editor.exe"),
        procs: vec![proc(100, 1_000, "synth-editor.exe")],
        ..World::default()
    }));
    let cfg = RecorderConfig { max_events_per_segment: 10_000, max_age: None, ..RecorderConfig::default() };
    let recorder = Recorder::open(dir.path(), &StaticKey([3u8; 32]), cfg).unwrap();
    let metrics = MetricsConfig { every_n_ticks: 1, min_cpu_permille: 0, min_working_set_kb: 0 };
    let engine = Engine::new(guard, metrics, recorder, FakeProcs(world.clone()), FakeCtx(world.clone()), 1);
    Rig { engine, world, _dir: dir }
}

fn cfg() -> GuardConfig {
    let mut c = GuardConfig::default();
    c.stability_window_ms = 1_000;
    c.excluded_apps.insert(ExeName::new("synth-excluded.exe").unwrap());
    c
}

/// Avança o relógio (mono e UTC caminham juntos, com origem em 1_000_000).
struct Clock(u64);
impl Clock {
    fn tick(&mut self, r: &mut Rig) -> bb_engine::TickReport {
        self.0 += 500;
        r.engine.tick(self.0, 1_000_000 + self.0 as i64).unwrap()
    }
    fn until_recording(&mut self, r: &mut Rig) {
        for _ in 0..10 {
            if self.tick(r).state == RecorderState::Recording {
                return;
            }
        }
        panic!("never reached Recording");
    }
    fn utc(&self) -> i64 {
        1_000_000 + self.0 as i64
    }
}

fn persisted_lines(r: &mut Rig) -> Vec<String> {
    r.engine.recorder_mut().seal().unwrap();
    let rec = r.engine.recorder();
    rec.list_segments().unwrap().iter().flat_map(|s| rec.read_segment(s.index).unwrap()).collect()
}

#[test]
fn records_process_start_and_metrics_once_the_context_is_stable() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    let lines = persisted_lines(&mut r);
    assert!(lines.iter().any(|l| l.contains("ProcessStarted") && l.contains("synth-editor.exe")));
    assert!(lines.iter().any(|l| l.contains("SystemMetrics")));
}

// Pausa manual — nada é consultado nem persistido.
#[test]
fn manual_pause_stops_collection_and_persistence_immediately() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    let before = persisted_lines(&mut r).len();
    let calls_before = r.world.borrow().proc_calls;

    r.engine.pause();
    r.world.borrow_mut().procs.push(proc(200, c.utc() + 10, "synth-new.exe"));
    for _ in 0..5 {
        let rep = c.tick(&mut r);
        assert_eq!((rep.state, rep.persisted), (RecorderState::ManualPause, 0));
    }
    assert_eq!(r.world.borrow().proc_calls, calls_before, "process source must not be queried while paused");
    assert_eq!(persisted_lines(&mut r).len(), before);
}

// O Guard segue avaliando durante a pausa manual.
#[test]
fn context_keeps_being_observed_during_manual_pause() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    r.engine.pause();
    let calls = r.world.borrow().ctx_calls;
    c.tick(&mut r);
    c.tick(&mut r);
    assert_eq!(r.world.borrow().ctx_calls, calls + 2);
}

// Retomar durante contexto sensível continua bloqueado; a retomada
// automática só vem após a janela de estabilidade.
#[test]
fn resume_during_a_sensitive_context_stays_blocked_and_collects_nothing() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    r.engine.pause();
    r.world.borrow_mut().foreground = Some("chrome.exe");
    c.tick(&mut r);
    let calls = r.world.borrow().proc_calls;

    let (state, reason) = r.engine.resume(c.0);
    assert_eq!((state, reason), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
    for _ in 0..3 {
        assert_eq!(c.tick(&mut r).state, RecorderState::PrivacyBlocked);
    }
    assert_eq!(r.world.borrow().proc_calls, calls);

    // volta a ser seguro: só grava depois da janela de estabilidade
    r.world.borrow_mut().foreground = Some("synth-editor.exe");
    assert_eq!(c.tick(&mut r).state, RecorderState::PrivacyBlocked);
    c.until_recording(&mut r);
}

#[test]
fn locked_session_blocks_collection() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    r.world.borrow_mut().locked = true;
    let calls = r.world.borrow().proc_calls;
    let rep = c.tick(&mut r);
    assert_eq!((rep.state, rep.reason), (RecorderState::PrivacyBlocked, ReasonCode::SessionLocked));
    assert_eq!(r.world.borrow().proc_calls, calls);
}

// Sem reconstrução: o que nasce e termina durante a pausa nunca aparece.
#[test]
fn processes_born_during_a_pause_are_never_reported() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    r.engine.pause();
    let born_at = c.utc() + 50;
    r.world.borrow_mut().procs.push(proc(555, born_at, "synth-secret-tool.exe"));
    c.tick(&mut r);
    c.tick(&mut r);
    r.engine.resume(c.0);
    c.until_recording(&mut r);
    // o processo continua vivo e depois termina
    c.tick(&mut r);
    r.world.borrow_mut().procs.retain(|p| p.key.pid != 555);
    c.tick(&mut r);
    c.tick(&mut r);
    let lines = persisted_lines(&mut r);
    assert!(!lines.iter().any(|l| l.contains("synth-secret-tool.exe")), "process from the pause leaked");
    assert!(!lines.iter().any(|l| l.contains("\"pid\":555")), "pid from the pause leaked");
}

#[test]
fn processes_started_during_an_initial_blocked_period_are_not_reported() {
    let mut r = rig(cfg());
    r.world.borrow_mut().foreground = Some("chrome.exe");
    let mut c = Clock(0);
    c.tick(&mut r);
    c.tick(&mut r);
    let born = c.utc() + 10;
    r.world.borrow_mut().procs.push(proc(777, born, "synth-early-bird.exe"));
    c.tick(&mut r);
    r.world.borrow_mut().foreground = Some("synth-editor.exe");
    c.until_recording(&mut r);
    let lines = persisted_lines(&mut r);
    assert!(!lines.iter().any(|l| l.contains("synth-early-bird.exe")));
    assert!(lines.iter().any(|l| l.contains("synth-editor.exe")), "pre-existing process is still baseline");
}

// Apps excluídos e protegidos nunca são persistidos, nem suas métricas e saída.
#[test]
fn excluded_and_protected_processes_are_never_persisted() {
    let mut r = rig(cfg());
    {
        let mut w = r.world.borrow_mut();
        w.procs.push(proc(300, 1_500, "synth-excluded.exe"));
        w.procs.push(proc(301, 1_600, "Chrome.exe"));
    }
    let mut c = Clock(0);
    c.until_recording(&mut r);
    c.tick(&mut r);
    r.world.borrow_mut().procs.retain(|p| p.key.pid < 300);
    c.tick(&mut r);
    let lines = persisted_lines(&mut r);
    assert!(!lines.iter().any(|l| l.contains("excluded") || l.contains("chrome")));
    assert!(!lines.iter().any(|l| l.contains("\"pid\":300") || l.contains("\"pid\":301")));
}

#[test]
fn collector_failure_surfaces_and_persists_nothing_more() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    let before = persisted_lines(&mut r).len();
    r.world.borrow_mut().fail_processes = true;
    c.0 += 500;
    assert!(r.engine.tick(c.0, c.utc()).is_err());
    assert_eq!(persisted_lines(&mut r).len(), before);
}

// Uma falha real nunca deixa o estado, e portanto o ícone, em "gravando".
#[test]
fn a_collection_failure_is_reflected_as_a_fault_and_recovers() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    assert_eq!(r.engine.state(c.0).0, RecorderState::Recording);

    r.world.borrow_mut().fail_processes = true;
    c.0 += 500;
    assert!(r.engine.tick(c.0, c.utc()).is_err());
    assert_eq!(r.engine.state(c.0), (RecorderState::SafetyFault, ReasonCode::RecorderFault));

    // a falha persiste enquanto a causa persistir
    c.0 += 500;
    assert!(r.engine.tick(c.0, c.utc()).is_err());
    assert_eq!(r.engine.state(c.0).0, RecorderState::SafetyFault);

    // recuperação: o primeiro ciclo bem-sucedido volta a gravar
    r.world.borrow_mut().fail_processes = false;
    let rep = c.tick(&mut r);
    assert_eq!(rep.state, RecorderState::Recording);
    assert_eq!(r.engine.state(c.0).0, RecorderState::Recording);
}

#[test]
fn a_fault_never_masks_a_manual_pause() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    r.world.borrow_mut().fail_processes = true;
    c.0 += 500;
    let _ = r.engine.tick(c.0, c.utc());
    r.engine.pause();
    assert_eq!(r.engine.state(c.0).0, RecorderState::ManualPause);
}

#[test]
fn shutdown_seals_and_stops_recording() {
    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    r.engine.shutdown().unwrap();
    assert_eq!(c.tick(&mut r).state, RecorderState::ShuttingDown);
    assert!(r.engine.recorder().verify().unwrap().events > 0);
}

// Só chaves da allowlist aparecem em disco.
#[test]
fn persisted_events_only_use_allowlisted_fields() {
    let allowed: BTreeSet<&str> = [
        "seq", "ts_utc_ms", "kind", "ProcessStarted", "key", "pid", "start_time_ms", "exe_name",
        "parent_pid", "ProcessExited", "exit_code", "ProcessMetrics", "cpu_permille",
        "working_set_kb", "SystemMetrics", "mem_used_kb", "mem_total_kb", "RecorderStateChanged",
        "state", "reason", "UserMarker", "code",
    ]
    .into_iter()
    .collect();

    let mut r = rig(cfg());
    let mut c = Clock(0);
    c.until_recording(&mut r);
    c.tick(&mut r);
    for line in persisted_lines(&mut r) {
        let mut keys = BTreeSet::new();
        collect_keys(&parse(&line), &mut keys);
        for k in keys {
            assert!(allowed.contains(k.as_str()), "unexpected field persisted: {k}");
        }
    }
}

// --- mini parser de chaves JSON (evita dependência extra nos testes) ---
enum J {
    Obj(Vec<(String, J)>),
    Other,
}

fn parse(s: &str) -> J {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    value(&b, &mut i)
}

fn value(b: &[char], i: &mut usize) -> J {
    match b[*i] {
        '{' => {
            *i += 1;
            let mut fields = Vec::new();
            while b[*i] != '}' {
                let key = string(b, i);
                *i += 1; // ':'
                fields.push((key, value(b, i)));
                if b[*i] == ',' {
                    *i += 1;
                }
            }
            *i += 1;
            J::Obj(fields)
        }
        '"' => {
            string(b, i);
            J::Other
        }
        _ => {
            while !matches!(b[*i], ',' | '}' | ']') {
                *i += 1;
            }
            J::Other
        }
    }
}

fn string(b: &[char], i: &mut usize) -> String {
    *i += 1;
    let mut out = String::new();
    while b[*i] != '"' {
        if b[*i] == '\\' {
            *i += 1;
        }
        out.push(b[*i]);
        *i += 1;
    }
    *i += 1;
    out
}

fn collect_keys(j: &J, out: &mut BTreeSet<String>) {
    if let J::Obj(fields) = j {
        for (k, v) in fields {
            out.insert(k.clone());
            collect_keys(v, out);
        }
    }
}
