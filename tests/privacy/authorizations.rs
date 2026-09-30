//! Testes do modo de teste (aplicações autorizadas). Dados sintéticos.
//!
//! Regras: autorização temporária, específica, revogável, só de dados técnicos, e em modo
//! restrito só o app autorizado é gravado.

use bb_core::{
    AuthError, EventKind, ExeName, GuardConfig, Observation, PrivacyGuard, ProcessKey, ReasonCode, RecorderState,
};

const WINDOW: u64 = 5_000;
const MINUTE: u64 = 60_000;

fn exe(s: &str) -> ExeName {
    ExeName::new(s).unwrap()
}

fn cfg() -> GuardConfig {
    let mut c = GuardConfig::default();
    c.stability_window_ms = WINDOW;
    c.max_staleness_ms = 100 * MINUTE;
    c
}

fn obs(fg: &str) -> Observation {
    Observation { detector_ok: true, session_locked: false, foreground: Some(exe(fg)) }
}

fn key(pid: u32) -> ProcessKey {
    ProcessKey { pid, start_time_ms: 1 }
}

fn started(pid: u32, name: &str) -> EventKind {
    EventKind::ProcessStarted { key: key(pid), exe_name: exe(name), parent_pid: 0 }
}

fn metrics(pid: u32) -> EventKind {
    EventKind::ProcessMetrics { key: key(pid), cpu_permille: 10, working_set_kb: 10 }
}

fn crash(name: &str) -> EventKind {
    EventKind::AppCrash { exe_name: exe(name), exception_code: 1 }
}

fn sys() -> EventKind {
    EventKind::SystemMetrics { cpu_permille: 1, mem_used_kb: 1, mem_total_kb: 2 }
}

/// Guard gravando normalmente com um app comum em primeiro plano. Devolve o instante atual.
fn recording(fg: &str) -> (PrivacyGuard, u64) {
    let mut g = PrivacyGuard::new(cfg());
    g.observe(0, obs(fg));
    g.observe(WINDOW + 1, obs(fg));
    assert_eq!(g.state(WINDOW + 1).0, RecorderState::Recording);
    (g, WINDOW + 1)
}

/// Autoriza `app` por `minutes` e faz esse app ficar em primeiro plano até estabilizar.
fn foreground_authorized(app: &str, minutes: u64) -> (PrivacyGuard, u64) {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe(app), minutes * MINUTE, true, true).unwrap();
    g.observe(t + 1, obs(app));
    let now = t + 1 + WINDOW;
    g.observe(now, obs(app));
    (g, now)
}

// As autorizações são temporárias e revogáveis.
#[test]
fn a_browser_in_the_foreground_blocks_recording_without_authorization() {
    let (mut g, t) = recording("synth-editor.exe");
    g.observe(t + 1, obs("chrome.exe"));
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
}

#[test]
fn authorization_allows_restricted_recording_after_the_stability_window() {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), 30 * MINUTE, true, true).unwrap();
    g.observe(t + 1, obs("chrome.exe"));
    assert_ne!(g.state(t + 1).0, RecorderState::Recording, "the stability window still applies");
    g.observe(t + 1 + WINDOW, obs("chrome.exe"));
    assert_eq!(g.state(t + 1 + WINDOW), (RecorderState::Recording, ReasonCode::RestrictedTest));
}

// O modo de teste não coleta nada além de dados técnicos do app autorizado.
#[test]
fn restricted_mode_admits_only_the_authorized_app() {
    let (mut g, t) = foreground_authorized("chrome.exe", 30);
    assert!(g.admit(t, 1, started(1, "chrome.exe")).is_some());
    assert!(g.admit(t, 2, metrics(1)).is_some());
    assert!(g.admit(t, 3, crash("chrome.exe")).is_some());

    assert!(g.admit(t, 4, started(2, "synth-editor.exe")).is_none(), "other apps must not be recorded");
    assert!(g.admit(t, 5, crash("synth-editor.exe")).is_none());
    assert!(g.admit(t, 6, sys()).is_none(), "no system metrics in restricted mode");
}

#[test]
fn authorized_app_events_are_recorded_even_when_another_app_is_in_the_foreground() {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), 30 * MINUTE, true, true).unwrap();
    g.observe(t + 1, obs("synth-editor.exe"));
    let now = t + 1 + WINDOW;
    g.observe(now, obs("synth-editor.exe"));
    assert_eq!(g.state(now).0, RecorderState::Recording);
    assert!(g.admit(now, 1, started(1, "chrome.exe")).is_some());
    assert!(g.admit(now, 2, started(2, "msedge.exe")).is_none(), "an unauthorized browser stays protected");
    assert!(g.admit(now, 3, started(3, "synth-editor.exe")).is_some());
}

#[test]
fn authorization_expires_on_its_own() {
    let (mut g, t) = foreground_authorized("chrome.exe", 5);
    assert!(g.admit(t, 1, started(1, "chrome.exe")).is_some());
    let after = t + 5 * MINUTE + 1;
    g.observe(after, obs("chrome.exe"));
    assert_eq!(g.state(after), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
    assert!(g.admit(after, 2, metrics(1)).is_none(), "tracked instance is dropped once the authorization is gone");
    assert!(g.authorizations(after).is_empty());
}

#[test]
fn expiry_also_stops_events_when_the_foreground_is_another_app() {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), MINUTE, true, true).unwrap();
    g.observe(t + 1, obs("synth-editor.exe"));
    let now = t + 1 + WINDOW;
    g.observe(now, obs("synth-editor.exe"));
    assert!(g.admit(now, 1, started(1, "chrome.exe")).is_some());
    let expired = t + MINUTE + 10;
    g.observe(expired, obs("synth-editor.exe"));
    assert_eq!(g.state(expired).0, RecorderState::Recording);
    assert!(g.admit(expired, 2, metrics(1)).is_none());
    assert!(g.admit(expired, 3, started(2, "chrome.exe")).is_none());
}

#[test]
fn revocation_takes_effect_immediately() {
    let (mut g, t) = foreground_authorized("chrome.exe", 30);
    assert!(g.admit(t, 1, started(1, "chrome.exe")).is_some());
    assert!(g.revoke_authorization(&exe("chrome.exe")));
    assert_eq!(g.state(t), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
    assert!(g.admit(t, 2, metrics(1)).is_none());
    assert!(!g.revoke_authorization(&exe("chrome.exe")), "nothing left to revoke");
}

#[test]
fn revoke_all_clears_every_authorization() {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), MINUTE, true, true).unwrap();
    g.authorize(t, exe("firefox.exe"), MINUTE, true, true).unwrap();
    g.revoke_all_authorizations();
    assert!(g.authorizations(t).is_empty());
}

#[test]
fn an_authorization_is_specific_to_one_executable() {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), 30 * MINUTE, true, true).unwrap();
    g.observe(t + 1, obs("msedge.exe"));
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
    g.observe(t + 2, obs("synth-editor.exe"));
    g.observe(t + 2 + WINDOW, obs("synth-editor.exe"));
    assert!(g.admit(t + 2 + WINDOW, 1, started(1, "msedge.exe")).is_none());
}

#[test]
fn only_protected_apps_can_be_authorized_and_inputs_are_validated() {
    let (mut g, t) = recording("synth-editor.exe");
    assert_eq!(g.authorize(t, exe("synth-editor.exe"), MINUTE, true, true), Err(AuthError::NotProtectedApp));
    assert_eq!(g.authorize(t, exe("chrome.exe"), MINUTE - 1, true, true), Err(AuthError::BadDuration));
    assert_eq!(g.authorize(t, exe("chrome.exe"), 9 * 3_600_000, true, true), Err(AuthError::BadDuration));
    assert_eq!(g.authorize(t, exe("chrome.exe"), MINUTE, false, false), Err(AuthError::NoSource));
    assert!(g.authorizations(t).is_empty(), "rejected requests leave no authorization behind");
}

#[test]
fn an_exclusion_rule_beats_an_authorization() {
    let (mut g, t) = foreground_authorized("chrome.exe", 30);
    assert!(g.admit(t, 1, started(1, "chrome.exe")).is_some());
    let mut c = cfg();
    c.excluded_apps.insert(exe("chrome.exe"));
    g.update_config(c);
    g.observe(t + 1, obs("chrome.exe"));
    assert_eq!(g.state(t + 1).0, RecorderState::PrivacyBlocked);
    g.observe(t + 2, obs("synth-editor.exe"));
    g.observe(t + 2 + WINDOW, obs("synth-editor.exe"));
    assert!(g.admit(t + 2 + WINDOW, 2, metrics(1)).is_none());
    assert_eq!(
        g.authorize(t + 2 + WINDOW, exe("chrome.exe"), MINUTE, true, true),
        Err(AuthError::ExcludedApp)
    );
}

#[test]
fn diagnostic_sources_are_chosen_independently() {
    // só métricas
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), 30 * MINUTE, true, false).unwrap();
    g.observe(t + 1, obs("synth-editor.exe"));
    let now = t + 1 + WINDOW;
    g.observe(now, obs("synth-editor.exe"));
    assert!(g.admit(now, 1, started(1, "chrome.exe")).is_some());
    assert!(g.admit(now, 2, crash("chrome.exe")).is_none(), "crashes were not authorized");

    // só falhas
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), 30 * MINUTE, false, true).unwrap();
    g.observe(t + 1, obs("synth-editor.exe"));
    let now = t + 1 + WINDOW;
    g.observe(now, obs("synth-editor.exe"));
    assert!(g.admit(now, 3, started(1, "chrome.exe")).is_none(), "process metrics were not authorized");
    assert!(g.admit(now, 4, crash("chrome.exe")).is_some());
}

// A autorização de um navegador nunca vira licença para outros apps sensíveis.
#[test]
fn a_password_manager_still_blocks_even_with_the_browser_authorized() {
    let (mut g, t) = foreground_authorized("chrome.exe", 30);
    g.observe(t + 1, obs("keepassxc.exe"));
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp));
    assert!(g.admit(t + 1, 1, started(1, "keepassxc.exe")).is_none());
}

#[test]
fn a_locked_session_still_blocks_in_restricted_mode() {
    let (mut g, t) = foreground_authorized("chrome.exe", 30);
    let mut o = obs("chrome.exe");
    o.session_locked = true;
    g.observe(t + 1, o);
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SessionLocked));
}

#[test]
fn leaving_the_authorized_browser_needs_a_fresh_stability_window() {
    let (mut g, t) = foreground_authorized("chrome.exe", 30);
    g.observe(t + 1, obs("synth-editor.exe"));
    assert_ne!(g.state(t + 1).0, RecorderState::Recording, "loosening the restriction is not immediate");
    assert!(g.admit(t + 1, 1, started(9, "synth-editor.exe")).is_none());
    g.observe(t + 1 + WINDOW, obs("synth-editor.exe"));
    assert_eq!(g.state(t + 1 + WINDOW), (RecorderState::Recording, ReasonCode::None));
    assert!(g.admit(t + 1 + WINDOW, 2, started(9, "synth-editor.exe")).is_some());
}

#[test]
fn reauthorizing_replaces_the_previous_authorization() {
    let (mut g, t) = recording("synth-editor.exe");
    g.authorize(t, exe("chrome.exe"), 5 * MINUTE, true, true).unwrap();
    g.authorize(t, exe("chrome.exe"), 60 * MINUTE, true, false).unwrap();
    let list = g.authorizations(t);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].expires_mono_ms, t + 60 * MINUTE);
    assert!(!list[0].allow_crashes);
}

#[test]
fn a_new_guard_has_no_authorizations_so_they_never_survive_a_restart() {
    let g = PrivacyGuard::new(cfg());
    assert!(g.authorizations(0).is_empty());
}
