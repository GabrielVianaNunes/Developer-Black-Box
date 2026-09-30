//! Lista de programas do PC (para as listas de privacidade), contra o Windows REAL. Sem dados gravados.
#![cfg(windows)]

use std::collections::HashSet;
use std::time::Instant;

use bb_collector::apps::{list_candidates, MAX_CANDIDATES};
use bb_core::ExeName;

#[test]
fn the_list_is_fast_valid_unique_and_contains_the_running_test_process() {
    let started = Instant::now();
    let list = list_candidates();
    let elapsed = started.elapsed();
    println!("{} candidates in {elapsed:?}", list.len());

    assert!(elapsed.as_secs_f64() < 2.0, "listing must feel instant, took {elapsed:?}");
    assert!(!list.is_empty() && list.len() <= MAX_CANDIDATES);

    // Cada entrada é um nome que o Guard aceitaria, em minúsculas e sem repetição.
    let mut seen = HashSet::new();
    for c in &list {
        assert!(c.exe.ends_with(".exe") && c.exe == c.exe.to_lowercase(), "{}", c.exe);
        assert!(ExeName::new(&c.exe).is_ok(), "{}", c.exe);
        assert!(!c.name.trim().is_empty());
        assert!(seen.insert(c.exe.clone()), "duplicate {}", c.exe);
    }

    // O próprio executável do teste está em execução agora.
    let me = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().to_lowercase();
    let mine = list.iter().find(|c| c.exe == me).unwrap_or_else(|| panic!("{me} not listed"));
    assert!(mine.running);

    // Programas instalados vêm antes dos que só estão em execução.
    let first_running_only = list.iter().position(|c| !c.installed);
    let last_installed = list.iter().rposition(|c| c.installed);
    if let (Some(r), Some(i)) = (first_running_only, last_installed) {
        assert!(i < r, "installed apps must be listed first");
    }
}

#[test]
fn a_second_listing_is_just_as_fast_and_has_the_same_installed_apps() {
    let a = list_candidates();
    let started = Instant::now();
    let b = list_candidates();
    assert!(started.elapsed().as_secs_f64() < 2.0);
    let installed = |l: &[bb_collector::apps::AppCandidate]| l.iter().filter(|c| c.installed).map(|c| c.exe.clone()).collect::<HashSet<_>>();
    assert_eq!(installed(&a), installed(&b), "the installed set is stable between calls");
}
