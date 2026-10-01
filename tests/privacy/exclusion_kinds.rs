//! Exclusão por tipo de evento no Guard. Somente dados sintéticos.

use bb_core::{
    AuthError, EventKind, ExclusionKind, ExclusionSet, ExeName, GuardConfig, Observation, PrivacyGuard, ProcessKey,
    ReasonCode, RecorderState,
};

const WINDOW: u64 = 5_000;
const RULE: &str = "synth-rule.exe";
const OTHER: &str = "synth-other.exe";

fn exe(s: &str) -> ExeName {
    ExeName::new(s).unwrap()
}

fn cfg_with(rule: Option<ExclusionSet>) -> GuardConfig {
    let mut c = GuardConfig::default();
    c.stability_window_ms = WINDOW;
    c.max_staleness_ms = 10_000;
    if let Some(set) = rule {
        c.partial_exclusions.insert(exe(RULE), set);
    }
    c
}

fn safe_obs() -> Observation {
    Observation { detector_ok: true, session_locked: false, foreground: Some(exe("synth-editor.exe")) }
}

fn chrome_obs() -> Observation {
    Observation { detector_ok: true, session_locked: false, foreground: Some(exe("chrome.exe")) }
}

fn key(pid: u32) -> ProcessKey {
    ProcessKey { pid, start_time_ms: 1_000 }
}

fn started(pid: u32, name: &str) -> EventKind {
    EventKind::ProcessStarted { key: key(pid), exe_name: exe(name), parent_pid: 1 }
}

fn metrics(pid: u32) -> EventKind {
    EventKind::ProcessMetrics { key: key(pid), cpu_permille: 10, working_set_kb: 1_000 }
}

fn exited(pid: u32) -> EventKind {
    EventKind::ProcessExited { key: key(pid), exit_code: Some(0) }
}

fn crash(name: &str) -> EventKind {
    EventKind::AppCrash { exe_name: exe(name), exception_code: 0xc000_0005 }
}

fn hang(name: &str) -> EventKind {
    EventKind::AppHang { exe_name: exe(name) }
}

/// Guard já gravando (primeiro plano seguro além da janela de estabilidade).
fn recording_guard(rule: Option<ExclusionSet>) -> (PrivacyGuard, u64) {
    let mut g = PrivacyGuard::new(cfg_with(rule));
    g.observe(0, safe_obs());
    let t = WINDOW + 1;
    g.observe(t, safe_obs());
    assert_eq!(g.state(t).0, RecorderState::Recording);
    (g, t)
}

/// Todos os conjuntos coerentes (início/fim excluído sempre leva CPU e memória junto).
fn every_set() -> Vec<ExclusionSet> {
    let mut out = Vec::new();
    for l in [false, true] {
        for m in [false, true] {
            for c in [false, true] {
                let s = ExclusionSet::new(l, m, c);
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

// Para cada combinação possível, um evento entra se e somente se o tipo dele NÃO está excluído.
#[test]
fn every_event_kind_is_admitted_exactly_when_its_kind_is_not_excluded() {
    for set in every_set() {
        let (mut g, t) = recording_guard(Some(set));
        let ok = |kind: ExclusionKind| !set.contains(kind);
        let label = format!("{set:?}");

        assert_eq!(g.admit(t, 1, started(1, RULE)).is_some(), ok(ExclusionKind::Lifecycle), "start {label}");
        assert_eq!(g.admit(t, 2, metrics(1)).is_some(), ok(ExclusionKind::Metrics) && ok(ExclusionKind::Lifecycle), "metrics {label}");
        assert_eq!(g.admit(t, 3, crash(RULE)).is_some(), ok(ExclusionKind::Crashes), "crash {label}");
        assert_eq!(g.admit(t, 4, hang(RULE)).is_some(), ok(ExclusionKind::Crashes), "hang {label}");
        assert_eq!(g.admit(t, 5, exited(1)).is_some(), ok(ExclusionKind::Lifecycle), "exit {label}");
    }
}

// Outros programas, métricas do sistema e marcadores não são afetados por uma regra de um programa.
#[test]
fn a_rule_only_touches_its_own_program() {
    let (mut g, t) = recording_guard(Some(ExclusionSet::ALL));
    assert!(g.admit(t, 1, started(10, OTHER)).is_some());
    assert!(g.admit(t, 2, metrics(10)).is_some());
    assert!(g.admit(t, 3, crash(OTHER)).is_some());
    assert!(g.admit(t, 4, EventKind::SystemMetrics { cpu_permille: 1, mem_used_kb: 1, mem_total_kb: 2 }).is_some());
    assert!(g.admit(t, 5, EventKind::UserMarker { code: 1 }).is_some());
    assert!(g.admit(t, 6, exited(10)).is_some());
    // E o programa da regra, excluído por inteiro, não grava nada.
    assert!(g.admit(t, 7, started(11, RULE)).is_none());
    assert!(g.admit(t, 8, crash(RULE)).is_none());
}

// Uma regra vazia não exclui nada; uma regra com tudo equivale a excluir o programa por inteiro.
#[test]
fn an_empty_rule_excludes_nothing_and_a_full_rule_excludes_everything() {
    let (mut none, t) = recording_guard(Some(ExclusionSet::NONE));
    assert!(none.admit(t, 1, started(1, RULE)).is_some());
    assert!(none.admit(t, 2, metrics(1)).is_some());
    assert!(none.admit(t, 3, crash(RULE)).is_some());

    let (mut all_partial, t) = recording_guard(Some(ExclusionSet::ALL));
    let mut whole = PrivacyGuard::new({
        let mut c = cfg_with(None);
        c.excluded_apps.insert(exe(RULE));
        c
    });
    whole.observe(0, safe_obs());
    whole.observe(t, safe_obs());
    for (i, ev) in [started(1, RULE), metrics(1), crash(RULE), hang(RULE), exited(1)].into_iter().enumerate() {
        let a = all_partial.admit(t, i as i64, ev.clone()).is_some();
        let b = whole.admit(t, i as i64, ev).is_some();
        assert!(!a && !b, "event {i}");
    }
}

// Só CPU e memória excluídas: início e fim continuam, e o fim é gravado mesmo depois de métricas descartadas.
#[test]
fn excluding_only_cpu_and_memory_keeps_the_lifecycle_and_still_tracks_the_instance() {
    let (mut g, t) = recording_guard(Some(ExclusionSet::new(false, true, false)));
    assert!(g.admit(t, 1, started(1, RULE)).is_some());
    for i in 0..5 {
        assert!(g.admit(t, 2 + i, metrics(1)).is_none(), "metrics {i} dropped");
    }
    assert!(g.admit(t, 10, exited(1)).is_some(), "the end is still recorded");
    assert!(g.admit(t, 11, exited(1)).is_none(), "and the instance is forgotten afterwards");
}

// Mudar a regra com o programa rodando vale já no próximo evento, nos dois sentidos.
#[test]
fn changing_the_rule_while_a_process_runs_applies_to_the_next_event() {
    let (mut g, t) = recording_guard(None);
    assert!(g.admit(t, 1, started(1, RULE)).is_some());
    assert!(g.admit(t, 2, metrics(1)).is_some());

    // Passa a excluir CPU e memória: as métricas param, o início/fim continua.
    g.update_config(cfg_with(Some(ExclusionSet::new(false, true, false))));
    g.observe(t + WINDOW + 1, safe_obs());
    let t2 = t + WINDOW + 1;
    assert!(g.admit(t2, 3, metrics(1)).is_none());
    // Tira a regra: as métricas da instância ainda acompanhada voltam.
    g.update_config(cfg_with(None));
    g.observe(t2 + 1, safe_obs());
    g.observe(t2 + WINDOW + 2, safe_obs());
    let t3 = t2 + WINDOW + 2;
    assert!(g.admit(t3, 4, metrics(1)).is_some());

    // Passa a excluir o início/fim: a instância deixa de ser acompanhada, e o fim não é gravado.
    g.update_config(cfg_with(Some(ExclusionSet::new(true, false, false))));
    g.observe(t3 + 1, safe_obs());
    g.observe(t3 + WINDOW + 2, safe_obs());
    let t4 = t3 + WINDOW + 2;
    assert!(g.admit(t4, 5, metrics(1)).is_none());
    assert!(g.admit(t4, 6, exited(1)).is_none());
    // Tirar a regra depois não ressuscita uma instância esquecida (falha fechada).
    g.update_config(cfg_with(None));
    g.observe(t4 + 1, safe_obs());
    g.observe(t4 + WINDOW + 2, safe_obs());
    assert!(g.admit(t4 + WINDOW + 2, 7, metrics(1)).is_none());
}

// Programa PROTEGIDO com autorização de teste: a exclusão parcial vence por fonte.
fn authorized_guard(rule: ExclusionSet) -> (PrivacyGuard, u64) {
    let mut c = cfg_with(None);
    c.partial_exclusions.insert(exe("chrome.exe"), rule);
    let mut g = PrivacyGuard::new(c);
    let r = g.authorize(0, exe("chrome.exe"), 3_600_000, true, true);
    r.expect("an authorization with a source that is not excluded is accepted");
    g.observe(0, chrome_obs());
    g.observe(WINDOW + 1, chrome_obs());
    (g, WINDOW + 1)
}

#[test]
fn an_authorized_protected_app_still_honours_the_kinds_it_excludes() {
    // Falhas excluídas: as métricas autorizadas entram, as falhas não.
    let (mut g, t) = authorized_guard(ExclusionSet::new(false, false, true));
    assert_eq!(g.state(t).0, RecorderState::Recording);
    assert!(g.admit(t, 1, started(1, "chrome.exe")).is_some());
    assert!(g.admit(t, 2, metrics(1)).is_some());
    assert!(g.admit(t, 3, crash("chrome.exe")).is_none());
    assert!(g.admit(t, 4, hang("chrome.exe")).is_none());

    // Só CPU e memória excluídas: início/fim e falhas entram, métricas não.
    let (mut g, t) = authorized_guard(ExclusionSet::new(false, true, false));
    assert!(g.admit(t, 1, started(2, "chrome.exe")).is_some());
    assert!(g.admit(t, 2, metrics(2)).is_none());
    assert!(g.admit(t, 3, crash("chrome.exe")).is_some());
}

#[test]
fn authorizing_a_source_that_is_entirely_excluded_is_refused() {
    let mut c = cfg_with(None);
    c.partial_exclusions.insert(exe("chrome.exe"), ExclusionSet::new(true, false, false)); // início/fim + CPU/memória
    let mut g = PrivacyGuard::new(c);
    assert_eq!(g.authorize(0, exe("chrome.exe"), 3_600_000, true, false), Err(AuthError::ExcludedApp));
    assert!(g.authorize(0, exe("chrome.exe"), 3_600_000, true, true).is_ok(), "crashes are still allowed");

    let mut c = cfg_with(None);
    c.partial_exclusions.insert(exe("chrome.exe"), ExclusionSet::new(false, false, true));
    let mut g = PrivacyGuard::new(c);
    assert_eq!(g.authorize(0, exe("chrome.exe"), 3_600_000, false, true), Err(AuthError::ExcludedApp));
    assert!(g.authorize(0, exe("chrome.exe"), 3_600_000, true, false).is_ok());
}

// Se a regra passa a excluir TUDO o que a autorização cobre, a autorização deixa de valer na hora.
#[test]
fn a_new_rule_that_excludes_everything_an_authorization_covers_cancels_its_effect() {
    let (mut g, t) = authorized_guard(ExclusionSet::new(false, false, true));
    assert_eq!(g.state(t).0, RecorderState::Recording);

    let mut c = cfg_with(None);
    c.partial_exclusions.insert(exe("chrome.exe"), ExclusionSet::ALL);
    g.update_config(c);
    g.observe(t + 1, chrome_obs());
    assert_eq!(g.state(t + 1), (RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp), "back to the protected-app block");
    assert!(g.admit(t + 1, 1, started(3, "chrome.exe")).is_none());
}
