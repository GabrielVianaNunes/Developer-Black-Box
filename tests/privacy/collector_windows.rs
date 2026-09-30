//! Teste de fumaça contra o Windows real. Só lê o próprio processo de teste e
//! propriedades gerais; nada é gravado.
#![cfg(windows)]

use bb_collector::{ContextSource, ProcessSource, WindowsContextSource, WindowsProcessSource};

#[test]
fn snapshot_contains_this_process_with_a_plausible_identity() {
    let mut src = WindowsProcessSource::new();
    let all = src.processes().unwrap();
    assert!(all.len() > 10, "expected a normal Windows process list, got {}", all.len());

    let me = all.iter().find(|p| p.key.pid == std::process::id()).expect("own process visible");
    assert!(me.exe_name.as_str().starts_with("collector_windows") || me.exe_name.as_str().starts_with("privacy_collector"));
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    assert!(me.key.start_time_ms <= now_ms && now_ms - me.key.start_time_ms < 3_600_000);
    assert!(me.working_set_kb > 0);
}

#[test]
fn process_names_never_contain_paths() {
    let mut src = WindowsProcessSource::new();
    for p in src.processes().unwrap() {
        let n = p.exe_name.as_str();
        assert!(!n.contains('\\') && !n.contains('/') && !n.contains(':'), "path leaked: {n}");
    }
}

#[test]
fn system_sample_is_coherent() {
    let mut src = WindowsProcessSource::new();
    let a = src.system().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let b = src.system().unwrap();
    assert!(a.mem_total_kb > 0 && a.mem_used_kb <= a.mem_total_kb);
    assert!(b.cpu_permille <= 1000);
}

#[test]
fn the_real_event_log_can_be_queried_and_yields_only_names_and_codes() {
    use bb_collector::{CrashKind, CrashSource, WindowsCrashSource};
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let since = now - 90 * 86_400_000;
    let recs = WindowsCrashSource::new().poll(since).expect("Application log is readable without admin rights");
    for r in &recs {
        assert!(r.ts_utc_ms >= since - 1_000 && r.ts_utc_ms <= now + 60_000, "timestamp in range");
        let n = r.exe_name.as_str();
        assert!(!n.contains('\\') && !n.contains('/'), "path leaked: {n}");
        if r.kind == CrashKind::Hang {
            assert!(r.exception_code.is_none());
        }
    }
    println!("real Event Log: {} crash/hang records in the last 90 days", recs.len());
    // uma consulta a partir do futuro não devolve nada
    assert!(WindowsCrashSource::new().poll(now + 86_400_000).unwrap().is_empty());
}

#[test]
fn context_observation_runs_and_reports_only_a_file_name() {
    let mut ctx = WindowsContextSource::new();
    let o = ctx.observe();
    assert!(o.detector_ok);
    if let Some(app) = o.foreground {
        assert!(!app.as_str().contains('\\'));
    }
    // `session_locked` pode ser true ou false dependendo do estado da máquina.
}
