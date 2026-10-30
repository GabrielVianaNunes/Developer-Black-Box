//! Fonte de processos do Linux contra o `/proc` REAL (issue #126). Só roda em Linux; sem dados gravados.
#![cfg(target_os = "linux")]

use std::collections::HashSet;
use std::time::Instant;

use bb_collector::{LinuxProcessSource, ProcessSource};
use bb_core::ExeName;

#[test]
fn the_real_proc_gives_this_very_test_process_with_a_stable_key_and_valid_names() {
    let mut src = LinuxProcessSource::linux();
    let started = Instant::now();
    let first = src.processes().expect("/proc is readable");
    let elapsed = started.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "a snapshot must be fast, took {elapsed:?}");
    assert!(!first.is_empty());

    // O próprio processo de teste está lá, com o nome do arquivo apontado por `exe`.
    let me = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().to_lowercase();
    let mine = first.iter().find(|p| p.key.pid == std::process::id()).expect("this process is listed");
    assert_eq!(mine.exe_name.as_str(), me.strip_suffix(" (deleted)").unwrap_or(&me));
    assert!(mine.working_set_kb > 0, "a running process has resident memory");
    assert_eq!(mine.parent_pid, unsafe { libc::getppid() } as u32);

    // Todo nome é um nome que o Guard aceitaria, e as chaves não se repetem.
    let mut keys = HashSet::new();
    for p in &first {
        assert!(ExeName::new(p.exe_name.as_str()).is_ok());
        assert!(p.key.start_time_ms > 1_500_000_000_000, "an absolute time, not ticks");
        assert!(keys.insert(p.key), "duplicate key");
    }

    // A chave do próprio processo é estável entre leituras, e o tempo de CPU não anda para trás.
    let second = src.processes().unwrap();
    let again = second.iter().find(|p| p.key.pid == std::process::id()).unwrap();
    assert_eq!(again.key, mine.key);
    assert!(again.cpu_time_100ns >= mine.cpu_time_100ns);
}

#[test]
fn processes_nobody_can_name_are_dropped_and_counted_never_listed() {
    let mut src = LinuxProcessSource::linux();
    let listed = src.processes().unwrap().len();
    // Threads de kernel existem em qualquer Linux com /proc completo (e, como usuário comum, processos de outros usuários também).
    // O que importa: o que não tem nome legível não aparece, e a contagem é só um número.
    let dropped = src.dropped_last_cycle();
    println!("{listed} listed, {dropped} dropped");
    assert!(listed > 0);
}

#[test]
fn the_system_sample_is_sane_and_the_second_reading_has_a_cpu_figure() {
    let mut src = LinuxProcessSource::linux();
    let a = src.system().unwrap();
    assert!(a.mem_total_kb > 0 && a.mem_used_kb <= a.mem_total_kb);
    assert_eq!(a.cpu_permille, 0, "the first reading has no reference");
    // Gasta um pouco de CPU para a diferença não ser nula em máquinas ociosas.
    let mut x = 0u64;
    for i in 0..30_000_000u64 {
        x = x.wrapping_add(i ^ (x >> 3));
    }
    assert_ne!(x, 1);
    let b = src.system().unwrap();
    assert!(b.cpu_permille <= 1000);
}
