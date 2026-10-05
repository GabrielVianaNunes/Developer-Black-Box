//! Exclusão com processos filhos ("excluir também o que este programa iniciou") no Guard. Somente dados sintéticos.
//!
//! A árvore é recalculada a cada ciclo a partir do instantâneo COMPLETO de processos (pai, filho, PID e horário de
//! início), antes dos eventos do ciclo. Garantias: filhos e netos de um programa excluído nunca chegam ao gravador
//! (início, CPU/memória e fim); outro processo com o MESMO nome fora da árvore continua gravado; PID reaproveitado não
//! herda a exclusão; um filho que perdeu o pai continua excluído; regra aplicada ou removida com processos já em
//! execução vale na hora; e a opção só vale para exclusão por INTEIRO.

use bb_core::{EventKind, ExclusionSet, ExeName, GuardConfig, Observation, PrivacyGuard, ProcessKey, ProcessRef, RecorderState};

const WINDOW: u64 = 5_000;
const ROOT: &str = "synth-root.exe";
const HELPER: &str = "synth-helper.exe";
const OTHER: &str = "synth-other.exe";

fn exe(s: &str) -> ExeName {
    ExeName::new(s).unwrap()
}

fn cfg(root_excluded: bool, tree: bool) -> GuardConfig {
    let mut c = GuardConfig::default();
    c.stability_window_ms = WINDOW;
    c.max_staleness_ms = 10_000;
    if root_excluded {
        c.excluded_apps.insert(exe(ROOT));
    }
    if tree {
        c.excluded_trees.insert(exe(ROOT));
    }
    c
}

fn safe_obs() -> Observation {
    Observation { detector_ok: true, session_locked: false, foreground: Some(exe("synth-editor.exe")) }
}

fn recording(c: GuardConfig) -> (PrivacyGuard, u64) {
    let mut g = PrivacyGuard::new(c);
    g.observe(0, safe_obs());
    let t = WINDOW + 1;
    g.observe(t, safe_obs());
    assert_eq!(g.state(t).0, RecorderState::Recording);
    (g, t)
}

/// (pid, horário de início, nome, pid do pai)
type P = (u32, i64, &'static str, u32);

fn snapshot(g: &mut PrivacyGuard, procs: &[P]) {
    let names: Vec<ExeName> = procs.iter().map(|p| exe(p.2)).collect();
    let refs: Vec<ProcessRef> = procs
        .iter()
        .zip(&names)
        .map(|(p, n)| ProcessRef { key: ProcessKey { pid: p.0, start_time_ms: p.1 }, exe: n, parent_pid: p.3 })
        .collect();
    g.observe_processes(&refs);
}

/// Aplica uma nova configuração e devolve o instante em que o Guard volta a gravar (a janela de estabilidade recomeça).
fn reconfigure(g: &mut PrivacyGuard, t: u64, c: GuardConfig) -> u64 {
    g.update_config(c);
    g.observe(t + 1, safe_obs());
    let again = t + 1 + WINDOW + 1;
    g.observe(again, safe_obs());
    assert_eq!(g.state(again).0, RecorderState::Recording, "recording again after the new rules");
    again
}

fn key(pid: u32, start: i64) -> ProcessKey {
    ProcessKey { pid, start_time_ms: start }
}

fn started(p: P) -> EventKind {
    EventKind::ProcessStarted { key: key(p.0, p.1), exe_name: exe(p.2), parent_pid: p.3 }
}

fn metrics(pid: u32, start: i64) -> EventKind {
    EventKind::ProcessMetrics { key: key(pid, start), cpu_permille: 10, working_set_kb: 1_000 }
}

fn exited(pid: u32, start: i64) -> EventKind {
    EventKind::ProcessExited { key: key(pid, start), exit_code: Some(0) }
}

fn admitted(g: &mut PrivacyGuard, t: u64, kind: EventKind) -> bool {
    g.admit(t, 1, kind).is_some()
}

const R: P = (100, 1_000, ROOT, 1);
const C: P = (200, 2_000, HELPER, 100);
const G: P = (300, 3_000, HELPER, 200);

#[test]
fn children_and_grandchildren_of_an_excluded_tree_are_never_recorded() {
    let (mut g, t) = recording(cfg(true, true));
    // A ordem dos processos no instantâneo não importa: o neto vem antes do pai e do avô.
    snapshot(&mut g, &[G, C, R]);
    for p in [R, C, G] {
        assert!(!admitted(&mut g, t, started(p)), "start of {}", p.0);
        assert!(!admitted(&mut g, t, metrics(p.0, p.1)), "metrics of {}", p.0);
        assert!(!admitted(&mut g, t, exited(p.0, p.1)), "exit of {}", p.0);
    }
}

#[test]
fn without_the_option_the_children_are_recorded_like_before() {
    let (mut g, t) = recording(cfg(true, false));
    snapshot(&mut g, &[R, C]);
    assert!(!admitted(&mut g, t, started(R)), "the program itself stays excluded");
    assert!(admitted(&mut g, t, started(C)));
    assert!(admitted(&mut g, t, metrics(C.0, C.1)));
    assert!(admitted(&mut g, t, exited(C.0, C.1)));
}

#[test]
fn a_process_with_the_same_name_outside_the_tree_is_still_recorded() {
    let (mut g, t) = recording(cfg(true, true));
    let outside: P = (900, 2_500, HELPER, 50);
    snapshot(&mut g, &[R, C, outside]);
    assert!(!admitted(&mut g, t, started(C)), "helper inside the tree");
    assert!(admitted(&mut g, t, started(outside)), "same name, other parent: recorded");
    assert!(admitted(&mut g, t, metrics(900, 2_500)));
    let its_child: P = (901, 2_600, OTHER, 900);
    snapshot(&mut g, &[R, C, outside, its_child]);
    assert!(admitted(&mut g, t, started(its_child)), "and its own children too");
}

#[test]
fn a_reused_pid_never_inherits_the_exclusion() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    assert!(!admitted(&mut g, t, started(C)));
    // O fim do processo raiz nunca foi visto; o Windows reaproveita o PID 100 para outro programa, que ganha um filho.
    let reused: P = (100, 9_000, OTHER, 1);
    let child_of_reused: P = (150, 9_500, HELPER, 100);
    snapshot(&mut g, &[reused, child_of_reused]);
    assert!(admitted(&mut g, t, started(reused)), "a new, unrelated process with the same pid");
    assert!(admitted(&mut g, t, started(child_of_reused)), "its child is not in the excluded tree");
    assert!(admitted(&mut g, t, metrics(150, 9_500)));
}

#[test]
fn a_child_that_claims_to_be_older_than_its_parent_is_not_a_descendant() {
    // Defesa contra PID reaproveitado: um filho não pode ter começado ANTES do pai.
    let (mut g, t) = recording(cfg(true, true));
    let root: P = (100, 5_000, ROOT, 1);
    let older: P = (200, 4_000, HELPER, 100);
    snapshot(&mut g, &[root, older]);
    assert!(!admitted(&mut g, t, started(root)));
    assert!(admitted(&mut g, t, started(older)), "started before the supposed parent: another process");
}

#[test]
fn a_child_that_lost_its_parent_stays_excluded_even_if_the_pid_is_reused() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    // O pai (raiz) termina e o PID 100 passa a ser de outro programa; o filho 200 continua vivo, apontando para 100.
    let reused: P = (100, 9_000, OTHER, 1);
    snapshot(&mut g, &[reused, C]);
    assert!(!admitted(&mut g, t, metrics(200, 2_000)), "the orphan was in the tree and still is");
    assert!(admitted(&mut g, t, started(reused)), "the unrelated process is recorded");
}

#[test]
fn a_new_child_of_an_orphan_is_excluded_too() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    snapshot(&mut g, &[C]); // a raiz saiu
    let grandchild: P = (400, 4_000, OTHER, 200);
    snapshot(&mut g, &[C, grandchild]);
    assert!(!admitted(&mut g, t, started(grandchild)));
}

#[test]
fn a_root_that_started_during_a_pause_still_covers_its_children_afterwards() {
    // A raiz nasceu durante uma pausa: o Differ a adota em silêncio e o Guard nunca vê o ProcessStarted dela. Como a
    // árvore vem do instantâneo, os filhos que aparecem depois da retomada continuam excluídos.
    let (mut g, t) = recording(cfg(true, true));
    g.pause_manual();
    g.resume_manual(t);
    let t2 = t + WINDOW + 1;
    g.observe(t2, safe_obs());
    let now = t2 + WINDOW + 1;
    g.observe(now, safe_obs());
    assert_eq!(g.state(now).0, RecorderState::Recording);
    snapshot(&mut g, &[R, C]);
    assert!(!admitted(&mut g, now, started(C)), "no ProcessStarted for the root was ever admitted");
    assert!(!admitted(&mut g, now, metrics(C.0, C.1)));
}

#[test]
fn a_rule_applied_with_the_tree_already_running_takes_effect_at_once() {
    let (mut g, t) = recording(cfg(false, false));
    let unrelated: P = (700, 1_500, OTHER, 1);
    snapshot(&mut g, &[R, C, unrelated]);
    assert!(admitted(&mut g, t, started(R)));
    assert!(admitted(&mut g, t, started(C)));
    assert!(admitted(&mut g, t, started(unrelated)));
    assert!(admitted(&mut g, t, metrics(C.0, C.1)), "recorded before the rule exists");
    // O usuário exclui o programa e a árvore dele; os processos já em execução deixam de ser gravados no ciclo seguinte.
    let now = reconfigure(&mut g, t, cfg(true, true));
    snapshot(&mut g, &[R, C, G, unrelated]);
    assert!(admitted(&mut g, now, metrics(unrelated.0, unrelated.1)), "control: an unrelated process is still recorded");
    assert!(!admitted(&mut g, now, metrics(R.0, R.1)), "the root");
    assert!(!admitted(&mut g, now, metrics(C.0, C.1)), "the child that was already running");
    assert!(!admitted(&mut g, now, started(G)), "a new grandchild of the running child");
    assert!(!admitted(&mut g, now, exited(C.0, C.1)));
}

#[test]
fn removing_the_rule_lets_new_processes_be_recorded_again() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    assert!(!admitted(&mut g, t, started(C)));
    let now = reconfigure(&mut g, t, cfg(false, false));
    let new_child: P = (300, 3_000, HELPER, 100);
    let new_grandchild: P = (400, 4_000, HELPER, 200);
    snapshot(&mut g, &[R, C, new_child, new_grandchild]);
    assert!(admitted(&mut g, now, started(new_child)), "a new child of the old root");
    assert!(admitted(&mut g, now, started(new_grandchild)), "a new child of the old child");
}

#[test]
fn turning_only_the_tree_option_off_keeps_the_program_itself_excluded() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    let now = reconfigure(&mut g, t, cfg(true, false));
    let new_child: P = (300, 3_000, HELPER, 100);
    snapshot(&mut g, &[R, C, new_child]);
    assert!(!admitted(&mut g, now, started(R)), "the program itself stays out");
    assert!(admitted(&mut g, now, started(new_child)), "new children are recorded again");
}

#[test]
fn the_option_only_counts_for_a_full_exclusion() {
    // Exclusão PARCIAL (só CPU/memória) + opção da árvore: a árvore NÃO é excluída.
    let mut c = cfg(false, true);
    c.partial_exclusions.insert(exe(ROOT), ExclusionSet::new(false, true, false));
    let (mut g, t) = recording(c);
    snapshot(&mut g, &[R, C]);
    assert!(admitted(&mut g, t, started(R)), "only its CPU and memory are left out");
    assert!(admitted(&mut g, t, started(C)), "children are not covered by a partial rule");
}

#[test]
fn the_tree_is_forgotten_when_its_processes_are_gone() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    snapshot(&mut g, &[]);
    let reused_c: P = (200, 8_000, HELPER, 100);
    snapshot(&mut g, &[reused_c]);
    assert!(admitted(&mut g, t, started(reused_c)), "pid 200 is a different process now");
}

#[test]
fn the_exit_of_an_old_unrelated_instance_is_still_recorded_when_its_pid_is_now_a_tree_member() {
    let (mut g, t) = recording(cfg(true, true));
    let old: P = (200, 2_000, OTHER, 1);
    snapshot(&mut g, &[R, old]);
    assert!(admitted(&mut g, t, started(old)), "an unrelated process, recorded");
    // O processo 200 termina e o Windows reaproveita o PID 200 para um FILHO da árvore excluída.
    let reused_in_tree: P = (200, 8_000, HELPER, 100);
    snapshot(&mut g, &[R, reused_in_tree]);
    assert!(!admitted(&mut g, t, started(reused_in_tree)), "the new instance is in the tree");
    assert!(admitted(&mut g, t, exited(200, 2_000)), "but the exit of the OLD instance (another start time) is not touched");
}

#[test]
fn removing_the_rule_frees_the_tree_even_before_the_next_snapshot() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C]);
    assert!(!admitted(&mut g, t, started(C)));
    let now = reconfigure(&mut g, t, cfg(false, false));
    // Sem nenhum instantâneo novo: a árvore antiga já não vale.
    assert!(admitted(&mut g, now, started(C)), "the stale tree must not outlive the rule");
}

// ---- contador de omitidos: só um número por regra ----

fn count_of(g: &PrivacyGuard, name: &str) -> u64 {
    g.omitted_counts().into_iter().find(|(e, _)| e.as_str() == name).map(|(_, n)| n).unwrap_or(0)
}

#[test]
fn the_counter_counts_what_an_exclusion_rule_left_out_per_rule() {
    let (mut g, t) = recording(cfg(true, true));
    snapshot(&mut g, &[R, C, G]);
    assert!(!admitted(&mut g, t, started(R)));
    assert!(!admitted(&mut g, t, started(C)), "child of the tree");
    assert!(!admitted(&mut g, t, started(G)), "grandchild of the tree");
    assert_eq!(count_of(&g, ROOT), 3, "the children count for the rule of the program they descend from");
    assert_eq!(count_of(&g, HELPER), 0, "the helper has no rule of its own");
    assert_eq!(g.omitted_counts().len(), 1);
}

#[test]
fn recorded_things_and_protected_apps_are_never_counted() {
    let mut c = cfg(true, false);
    c.protected_apps.insert(exe("synth-bank.exe"));
    let (mut g, t) = recording(c);
    snapshot(&mut g, &[R, C]);
    assert!(admitted(&mut g, t, started(C)), "no tree option: the child is recorded");
    assert!(!admitted(&mut g, t, started((400, 4_000, "synth-bank.exe", 1))));
    assert_eq!(count_of(&g, HELPER), 0);
    assert_eq!(count_of(&g, "synth-bank.exe"), 0, "protected is not an exclusion rule");
    assert_eq!(g.omitted_counts().len(), 0);
}

#[test]
fn a_partial_rule_counts_only_the_omitted_kind() {
    let mut c = GuardConfig::default();
    c.stability_window_ms = WINDOW;
    c.max_staleness_ms = 10_000;
    c.partial_exclusions.insert(exe(OTHER), ExclusionSet::new(false, false, true));
    let (mut g, t) = recording(c);
    assert!(admitted(&mut g, t, started((500, 5_000, OTHER, 1))), "lifecycle is still recorded");
    assert_eq!(count_of(&g, OTHER), 0);
    let crash = EventKind::AppCrash { exe_name: exe(OTHER), exception_code: 1 };
    assert!(!admitted(&mut g, t, crash));
    assert_eq!(count_of(&g, OTHER), 1);
}

#[test]
fn removing_a_rule_drops_its_counter() {
    let (mut g, t) = recording(cfg(true, false));
    assert!(!admitted(&mut g, t, started(R)));
    assert_eq!(count_of(&g, ROOT), 1);
    g.update_config(cfg(false, false));
    assert_eq!(g.omitted_counts().len(), 0, "the counter belongs to the rule");
}
