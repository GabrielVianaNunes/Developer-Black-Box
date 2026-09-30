//! Testes de privacidade do Guard. Somente dados sintéticos.

use bb_core::{
    EventKind, ExeName, GuardConfig, Observation, PrivacyGuard, ProcessKey, ReasonCode,
    RecorderState,
};

const WINDOW: u64 = 5_000;

fn exe(s: &str) -> ExeName {
    ExeName::new(s).unwrap()
}

fn cfg() -> GuardConfig {
    let mut c = GuardConfig::default();
    c.stability_window_ms = WINDOW;
    c.max_staleness_ms = 10_000;
    c.excluded_apps.insert(exe("synth-excluded.exe"));
    c
}

fn safe_obs() -> Observation {
    Observation { detector_ok: true, session_locked: false, foreground: Some(exe("synth-editor.exe")) }
}

fn protected_obs() -> Observation {
    Observation { detector_ok: true, session_locked: false, foreground: Some(exe("chrome.exe")) }
}

fn key(pid: u32) -> ProcessKey {
    ProcessKey { pid, start_time_ms: 1_000 }
}

fn started(pid: u32, name: &str) -> EventKind {
    EventKind::ProcessStarted { key: key(pid), exe_name: exe(name), parent_pid: 1 }
}

/// Guard já gravando: observação segura mantida além da janela de estabilidade.
fn recording_guard() -> (PrivacyGuard, u64) {
    let mut g = PrivacyGuard::new(cfg());
    g.observe(0, safe_obs());
    let t = WINDOW + 1;
    g.observe(t, safe_obs());
    assert_eq!(g.state(t).0, RecorderState::Recording);
    (g, t)
}

#[test]
fn nothing_is_recorded_before_first_observation() {
    let mut g = PrivacyGuard::new(cfg());
    assert_eq!(g.state(0).0, RecorderState::Starting);
    assert!(g.admit(0, 0, started(1, "synth-editor.exe")).is_none());
}

#[test]
fn safe_context_needs_the_stability_window() {
    let mut g = PrivacyGuard::new(cfg());
    g.observe(0, safe_obs());
    assert_eq!(g.state(0).0, RecorderState::PrivacyBlocked);
    g.observe(WINDOW - 1, safe_obs());
    assert_eq!(g.state(WINDOW - 1).0, RecorderState::PrivacyBlocked);
    g.observe(WINDOW, safe_obs());
    assert_eq!(g.state(WINDOW).0, RecorderState::Recording);
}

// Pausa manual interrompe e nada é registrado durante a pausa.
#[test]
fn manual_pause_stops_recording_and_persists_nothing() {
    let (mut g, t) = recording_guard();
    assert!(g.admit(t, 1, started(1, "synth-editor.exe")).is_some());
    g.pause_manual();
    assert_eq!(g.state(t), (RecorderState::ManualPause, ReasonCode::ManualPause));
    assert!(g.admit(t, 2, started(2, "synth-editor.exe")).is_none());
    assert!(g.admit(t, 3, EventKind::SystemMetrics { cpu_permille: 1, mem_used_kb: 1, mem_total_kb: 2 }).is_none());
    assert!(g.admit(t, 4, EventKind::UserMarker { code: 1 }).is_none());
}

// O Guard continua avaliando durante a pausa manual.
#[test]
fn guard_keeps_evaluating_while_manually_paused() {
    let (mut g, t) = recording_guard();
    g.pause_manual();
    g.observe(t + 100, protected_obs());
    // Ao retomar, o contexto sensível observado durante a pausa bloqueia.
    let (state, reason) = g.resume_manual(t + 100);
    assert_eq!(state, RecorderState::PrivacyBlocked);
    assert_eq!(reason, ReasonCode::SensitiveApp);
}

// Retomada manual respeita o Guard.
#[test]
fn manual_resume_does_not_override_a_sensitive_context() {
    let (mut g, t) = recording_guard();
    g.pause_manual();
    g.observe(t + 1, protected_obs());
    let (state, _) = g.resume_manual(t + 1);
    assert_ne!(state, RecorderState::Recording);
    assert!(g.admit(t + 1, 5, started(9, "synth-editor.exe")).is_none());
}

#[test]
fn manual_resume_works_when_context_is_stably_safe() {
    let (mut g, t) = recording_guard();
    g.pause_manual();
    g.observe(t + 1, safe_obs());
    let (state, reason) = g.resume_manual(t + 1);
    assert_eq!((state, reason), (RecorderState::Recording, ReasonCode::None));
}

// Retomada automática não ocorre em contexto sensível e exige estabilidade.
#[test]
fn automatic_resume_waits_for_stability_after_a_sensitive_context() {
    let (mut g, t) = recording_guard();
    g.observe(t + 1, protected_obs());
    assert_eq!(g.state(t + 1).0, RecorderState::PrivacyBlocked);
    // Volta a ser seguro, mas a janela recomeça.
    let s = t + 2;
    g.observe(s, safe_obs());
    assert_eq!(g.state(s).0, RecorderState::PrivacyBlocked);
    g.observe(s + WINDOW - 1, safe_obs());
    assert_eq!(g.state(s + WINDOW - 1).0, RecorderState::PrivacyBlocked);
    g.observe(s + WINDOW, safe_obs());
    assert_eq!(g.state(s + WINDOW).0, RecorderState::Recording);
}

#[test]
fn flapping_context_resets_the_stability_window() {
    let (mut g, t) = recording_guard();
    g.observe(t + 1, protected_obs());
    g.observe(t + 2, safe_obs());
    g.observe(t + 3_000, protected_obs());
    g.observe(t + 3_001, safe_obs());
    let now = t + 3_001 + WINDOW - 1;
    g.observe(now, safe_obs());
    assert_eq!(g.state(now).0, RecorderState::PrivacyBlocked);
}

#[test]
fn locked_session_blocks_recording() {
    let (mut g, t) = recording_guard();
    let mut o = safe_obs();
    o.session_locked = true;
    g.observe(t + 1, o);
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SessionLocked));
}

#[test]
fn unknown_foreground_is_not_treated_as_safe() {
    let (mut g, t) = recording_guard();
    let mut o = safe_obs();
    o.foreground = None;
    g.observe(t + 1, o);
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::ContextUnknown));
}

// Aplicativos excluídos ou protegidos não têm eventos persistidos.
#[test]
fn excluded_and_protected_apps_are_never_persisted() {
    let (mut g, t) = recording_guard();
    assert!(g.admit(t, 1, started(10, "synth-excluded.exe")).is_none());
    assert!(g.admit(t, 2, started(11, "Chrome.exe")).is_none());
    assert!(g.admit(t, 3, started(12, "synth-editor.exe")).is_some());
}

#[test]
fn metrics_and_exit_of_dropped_instances_are_dropped_too() {
    let (mut g, t) = recording_guard();
    assert!(g.admit(t, 1, started(10, "synth-excluded.exe")).is_none());
    let m = EventKind::ProcessMetrics { key: key(10), cpu_permille: 5, working_set_kb: 10 };
    assert!(g.admit(t, 2, m).is_none());
    assert!(g.admit(t, 3, EventKind::ProcessExited { key: key(10), exit_code: Some(0) }).is_none());
}

#[test]
fn metrics_of_instances_never_seen_starting_are_dropped() {
    let (mut g, t) = recording_guard();
    let m = EventKind::ProcessMetrics { key: key(77), cpu_permille: 5, working_set_kb: 10 };
    assert!(g.admit(t, 1, m).is_none());
}

#[test]
fn metrics_of_allowed_instances_are_admitted_until_exit() {
    let (mut g, t) = recording_guard();
    assert!(g.admit(t, 1, started(20, "synth-editor.exe")).is_some());
    let m = EventKind::ProcessMetrics { key: key(20), cpu_permille: 5, working_set_kb: 10 };
    assert!(g.admit(t, 2, m.clone()).is_some());
    assert!(g.admit(t, 3, EventKind::ProcessExited { key: key(20), exit_code: None }).is_some());
    assert!(g.admit(t, 4, m).is_none());
}

// Falha do detector impede a gravação.
#[test]
fn detector_failure_stops_recording() {
    let (mut g, t) = recording_guard();
    let mut o = safe_obs();
    o.detector_ok = false;
    g.observe(t + 1, o);
    assert_eq!(g.state(t + 1), (RecorderState::SafetyFault, ReasonCode::DetectorUnavailable));
    assert!(g.admit(t + 1, 1, started(30, "synth-editor.exe")).is_none());
}

#[test]
fn stale_observations_stop_recording() {
    let (mut g, t) = recording_guard();
    let later = t + 10_001;
    assert_eq!(g.state(later).0, RecorderState::SafetyFault);
    assert!(g.admit(later, 1, started(31, "synth-editor.exe")).is_none());
}

#[test]
fn recovery_after_a_stale_gap_requires_a_fresh_stability_window() {
    let (mut g, t) = recording_guard();
    let back = t + 20_000;
    g.observe(back, safe_obs());
    assert_ne!(g.state(back).0, RecorderState::Recording);
    g.observe(back + WINDOW, safe_obs());
    assert_eq!(g.state(back + WINDOW).0, RecorderState::Recording);
}

// Regras editadas em tempo de execução valem na hora.
#[test]
fn a_new_exclusion_revokes_already_tracked_instances() {
    let (mut g, t) = recording_guard();
    assert!(g.admit(t, 1, started(60, "synth-editor.exe")).is_some());
    let metrics = EventKind::ProcessMetrics { key: key(60), cpu_permille: 1, working_set_kb: 1 };
    assert!(g.admit(t, 2, metrics.clone()).is_some());

    let mut c = cfg();
    c.excluded_apps.insert(exe("synth-editor.exe"));
    g.update_config(c);
    g.observe(t + 1, safe_obs());
    let later = t + 1 + WINDOW;
    g.observe(later, safe_obs());
    assert_eq!(g.state(later).0, RecorderState::Recording);
    assert!(g.admit(later, 3, metrics).is_none(), "metrics of a now-excluded app must stop");
    assert!(g.admit(later, 4, EventKind::ProcessExited { key: key(60), exit_code: None }).is_none());
    assert!(g.admit(later, 5, started(61, "synth-editor.exe")).is_none());
}

#[test]
fn a_config_change_requires_a_fresh_stability_window() {
    let (mut g, t) = recording_guard();
    g.update_config(cfg());
    assert_ne!(g.state(t).0, RecorderState::Recording);
    g.observe(t + 1, safe_obs());
    assert_ne!(g.state(t + 1).0, RecorderState::Recording);
    g.observe(t + 1 + WINDOW, safe_obs());
    assert_eq!(g.state(t + 1 + WINDOW).0, RecorderState::Recording);
}

#[test]
fn newly_protected_foreground_app_blocks_immediately() {
    let (mut g, t) = recording_guard();
    let mut c = cfg();
    c.protected_apps.insert(exe("synth-editor.exe"));
    g.update_config(c);
    g.observe(t + 1, safe_obs()); // o primeiro plano é o app recém-protegido
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
}

// Falhas do Windows Event Log seguem as mesmas regras de app protegido/excluído.
#[test]
fn crash_and_hang_events_respect_protected_and_excluded_apps() {
    let (mut g, t) = recording_guard();
    let crash = |n: &str| EventKind::AppCrash { exe_name: exe(n), exception_code: 0xc000_0005 };
    let hang = |n: &str| EventKind::AppHang { exe_name: exe(n) };
    assert!(g.admit(t, 1, crash("synth-editor.exe")).is_some());
    assert!(g.admit(t, 2, hang("synth-editor.exe")).is_some());
    assert!(g.admit(t, 3, crash("synth-excluded.exe")).is_none());
    assert!(g.admit(t, 4, hang("synth-excluded.exe")).is_none());
    assert!(g.admit(t, 5, crash("Chrome.exe")).is_none());
    assert!(g.admit(t, 6, hang("chrome.exe")).is_none());
}

#[test]
fn crash_events_are_not_admitted_while_paused() {
    let (mut g, t) = recording_guard();
    g.pause_manual();
    let crash = EventKind::AppCrash { exe_name: exe("synth-editor.exe"), exception_code: 1 };
    assert!(g.admit(t, 1, crash).is_none());
}

#[test]
fn shutdown_blocks_everything() {
    let (mut g, t) = recording_guard();
    g.begin_shutdown();
    assert_eq!(g.state(t).0, RecorderState::ShuttingDown);
    assert!(g.admit(t, 1, started(40, "synth-editor.exe")).is_none());
}

#[test]
fn admitted_events_get_increasing_sequence_numbers() {
    let (mut g, t) = recording_guard();
    let a = g.admit(t, 1, started(50, "synth-editor.exe")).unwrap();
    let b = g.admit(t, 2, started(51, "synth-editor.exe")).unwrap();
    assert!(b.seq() > a.seq());
}
