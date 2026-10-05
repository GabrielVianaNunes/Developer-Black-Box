//! Lista de programas do PC (para as listas de privacidade), contra o Windows REAL. Sem dados gravados.
#![cfg(windows)]

use std::collections::HashSet;
use std::time::{Duration, Instant};

use bb_collector::apps::{list_candidates, MAX_CANDIDATES};
use bb_core::ExeName;

/// Roda `f` três vezes e devolve o resultado da última com a duração da MAIS RÁPIDA. Um pico de carga da máquina (CI, antivírus)
/// atrasa uma execução e não pode derrubar o teste; uma lentidão de verdade atrasa as três.
fn fastest_of_three<T>(mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut last = None;
    for _ in 0..3 {
        let started = Instant::now();
        let out = f();
        best = best.min(started.elapsed());
        last = Some(out);
    }
    (best, last.expect("ran three times"))
}

#[test]
fn the_list_is_fast_valid_unique_and_contains_the_running_test_process() {
    let (elapsed, list) = fastest_of_three(list_candidates);
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
    let (elapsed, b) = fastest_of_three(list_candidates);
    assert!(elapsed.as_secs_f64() < 2.0, "second listing took {elapsed:?}");
    let installed = |l: &[bb_collector::apps::AppCandidate]| l.iter().filter(|c| c.installed).map(|c| c.exe.clone()).collect::<HashSet<_>>();
    assert_eq!(installed(&a), installed(&b), "the installed set is stable between calls");
}

#[test]
fn the_start_menu_adds_real_programs_with_friendly_names_quickly() {
    let (took, found) = fastest_of_three(bb_collector::apps::start_menu_programs);
    println!("{} start menu programs in {took:?}", found.len());

    assert!(took.as_secs_f64() < 2.0, "reading the shortcuts must feel instant, took {took:?}");
    assert!(!found.is_empty(), "a Windows user always has some Start Menu shortcuts");
    for (exe, name) in &found {
        assert!(exe.ends_with(".exe") && exe == &exe.to_lowercase() && ExeName::new(exe).is_ok(), "{exe}");
        assert!(!name.trim().is_empty());
        assert!(!exe.contains("uninst"), "uninstallers must be filtered: {exe}");
    }

    // O seletor mostra esses nomes (quando um mesmo .exe tem vários atalhos, vale o primeiro).
    let list = list_candidates();
    let with_shortcut_name = found.iter().filter(|(e, n)| list.iter().any(|c| &c.exe == e && &c.name == n)).count();
    assert!(with_shortcut_name * 2 >= found.len(), "{with_shortcut_name} of {}", found.len());
    assert!(list.iter().all(|c| c.installed || c.running));
}

#[test]
fn store_apps_are_listed_as_installed_with_valid_names_and_fast() {
    use bb_collector::apps::store_packages;
    let (elapsed, store) = fastest_of_three(store_packages);
    println!("{} store entries in {elapsed:?}", store.len());
    assert!(elapsed.as_secs_f64() < 2.0, "store listing must stay fast, took {elapsed:?}");
    for (exe, name) in &store {
        assert!(exe.ends_with(".exe") && exe == &exe.to_lowercase() && ExeName::new(exe).is_ok(), "{exe}");
        assert!(!name.trim().is_empty() && !name.starts_with('@') && !name.to_lowercase().starts_with("ms-resource:"), "{exe}: {name}");
        assert!(!exe.contains(['\\', '/']), "{exe}");
    }
    // Todo app da Loja encontrado também aparece na lista final, como instalado.
    let list = list_candidates();
    for (exe, _) in store.iter().take(50) {
        let c = list.iter().find(|c| &c.exe == exe).unwrap_or_else(|| panic!("{exe} missing from the list"));
        assert!(c.installed, "{exe} must count as installed");
    }
}

/// Sonda manual (ignorada na suíte): imprime os apps da Loja encontrados neste PC.
#[test]
#[ignore]
fn probe_print_store_apps() {
    for (exe, name) in bb_collector::apps::store_packages() {
        println!("STORE {exe} | {name}");
    }
}
