//! Testes de privacidade e integridade do Recorder. Somente dados sintéticos.

use std::fs;
use std::path::Path;

use bb_core::{EventKind, ExeName, GuardConfig, Observation, PrivacyGuard, ProcessKey, ValidatedEvent};
use bb_recorder::{Recorder, RecorderConfig, RecorderError, StaticKey};

const MARKER: &str = "synth-marker-app.exe";

fn keys() -> StaticKey {
    StaticKey([7u8; 32])
}

fn recording_guard() -> (PrivacyGuard, u64) {
    let mut g = PrivacyGuard::new(GuardConfig::default());
    let obs = Observation { detector_ok: true, session_locked: false, foreground: Some(ExeName::new("synth-editor.exe").unwrap()) };
    g.observe(0, obs.clone());
    g.observe(6_000, obs);
    (g, 6_000)
}

/// Eventos válidos, criados pelo único caminho possível: o Guard.
fn events(n: usize) -> Vec<ValidatedEvent> {
    let (mut g, t) = recording_guard();
    (0..n)
        .map(|i| {
            let key = ProcessKey { pid: i as u32 + 1, start_time_ms: 1 };
            let kind = EventKind::ProcessStarted { key, exe_name: ExeName::new(MARKER).unwrap(), parent_pid: 0 };
            g.admit(t, i as i64, kind).expect("guard admits synthetic event")
        })
        .collect()
}

fn small_cfg() -> RecorderConfig {
    RecorderConfig { max_events_per_segment: 3, max_age: None, ..RecorderConfig::default() }
}

fn all_bytes(dir: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            out.extend(fs::read(p).unwrap());
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn total_events(r: &Recorder) -> usize {
    r.list_segments().unwrap().iter().map(|s| r.read_segment(s.index).unwrap().len()).sum()
}

#[test]
fn append_seal_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(7) {
        r.append(&e).unwrap();
    }
    r.seal().unwrap();
    assert_eq!(total_events(&r), 7);
    assert_eq!(r.list_segments().unwrap().len(), 3); // 3 + 3 + 1
    let rep = r.verify().unwrap();
    assert_eq!((rep.segments, rep.events), (3, 7));
}

// Nada sensível/legível em disco, nem no journal nem nos segmentos.
#[test]
fn evidence_is_encrypted_at_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    let evs = events(4);
    for e in &evs[..2] {
        r.append(e).unwrap();
    }
    // journal ainda aberto: já está cifrado
    assert!(!contains(&all_bytes(dir.path()), MARKER));
    for e in &evs[2..] {
        r.append(e).unwrap();
    }
    r.seal().unwrap();
    let bytes = all_bytes(dir.path());
    assert!(!contains(&bytes, MARKER));
    assert!(!contains(&bytes, "exe_name"));
}

#[test]
fn wrong_key_cannot_read_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(3) {
        r.append(&e).unwrap();
    }
    drop(r);
    let other = Recorder::open(dir.path(), &StaticKey([9u8; 32]), small_cfg()).unwrap();
    assert!(matches!(other.verify(), Err(RecorderError::Crypto)));
}

#[test]
fn tampered_segment_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(3) {
        r.append(&e).unwrap();
    }
    let path = dir.path().join("seg-0000000000.bbseg");
    let mut b = fs::read(&path).unwrap();
    let last = b.len() - 1;
    b[last] ^= 1;
    fs::write(&path, b).unwrap();
    assert!(matches!(r.verify(), Err(RecorderError::Crypto)));
}

#[test]
fn missing_middle_segment_breaks_the_chain_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(9) {
        r.append(&e).unwrap();
    }
    fs::remove_file(dir.path().join("seg-0000000001.bbseg")).unwrap();
    assert!(matches!(r.verify(), Err(RecorderError::Corrupt("missing segment"))));
}

#[test]
fn chain_continues_across_sessions() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
        for e in events(3) {
            r.append(&e).unwrap();
        }
    }
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(3) {
        r.append(&e).unwrap();
    }
    assert_eq!(r.verify().unwrap().segments, 2);
}

// Recuperação após encerramento inesperado.
#[test]
fn journal_is_recovered_after_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(2) {
        r.append(&e).unwrap();
    }
    std::mem::forget(r); // simula queda: sem seal, sem Drop
    let r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    let rep = r.recovery_report().unwrap();
    assert_eq!((rep.recovered_events, rep.discarded_tail), (2, false));
    assert_eq!(total_events(&r), 2);
    assert!(r.verify().is_ok());
    assert!(!dir.path().join("journal.bbwal").exists());
}

#[test]
fn truncated_journal_tail_is_discarded_and_the_rest_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(2) {
        r.append(&e).unwrap();
    }
    std::mem::forget(r);
    let jp = dir.path().join("journal.bbwal");
    let b = fs::read(&jp).unwrap();
    fs::write(&jp, &b[..b.len() - 5]).unwrap(); // corta o final do último quadro
    let r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    let rep = r.recovery_report().unwrap();
    assert_eq!((rep.recovered_events, rep.discarded_tail), (1, true));
    assert!(r.verify().is_ok());
}

#[test]
fn corrupt_frame_in_the_middle_keeps_the_file_for_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(2) {
        r.append(&e).unwrap(); // abaixo do limite de 3: o journal continua aberto
    }
    std::mem::forget(r);
    let jp = dir.path().join("journal.bbwal");
    let mut b = fs::read(&jp).unwrap();
    b[24 + 4 + 12 + 2] ^= 0xff; // dentro do 1º quadro
    fs::write(&jp, b).unwrap();
    let r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    assert_eq!(r.recovery_report().unwrap().recovered_events, 0);
    assert!(dir.path().join("journal.bbwal.corrupt").exists());
}

// Falha ao gravar não corrompe evidências existentes nem perde o journal.
#[test]
fn failed_seal_keeps_existing_evidence_and_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Recorder::open(dir.path(), &keys(), small_cfg()).unwrap();
    for e in events(3) {
        r.append(&e).unwrap(); // gera o segmento 0
    }
    // Um diretório no lugar do próximo arquivo faz o rename falhar.
    let blocker = dir.path().join("seg-0000000001.bbseg");
    fs::create_dir(&blocker).unwrap();
    for e in events(2) {
        r.append(&e).unwrap();
    }
    assert!(r.seal().is_err());
    assert!(r.verify().is_ok());
    assert!(dir.path().join("journal.bbwal").exists());
    assert!(!dir.path().join("seg-0000000001.bbseg.tmp").exists());
    fs::remove_dir(&blocker).unwrap();
    r.seal().unwrap();
    assert_eq!(total_events(&r), 5);
    assert!(r.verify().is_ok());
}

#[test]
fn quota_full_of_preserved_evidence_refuses_new_events_without_damage() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = RecorderConfig { max_total_bytes: 700, ..small_cfg() };
    let mut r = Recorder::open(dir.path(), &keys(), cfg).unwrap();
    let evs = events(40);
    let mut refused = false;
    for e in &evs {
        match r.append(e) {
            Ok(()) => {
                for s in r.list_segments().unwrap() {
                    r.preserve(s.index).unwrap();
                }
            }
            Err(RecorderError::QuotaExceeded) => {
                refused = true;
                break;
            }
            Err(other) => panic!("unexpected: {other}"),
        }
    }
    assert!(refused, "recorder should refuse once quota is full of preserved segments");
    assert!(r.verify().is_ok());
}

#[test]
fn retention_removes_oldest_unpreserved_and_keeps_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = RecorderConfig { max_total_bytes: 900, ..small_cfg() };
    let mut r = Recorder::open(dir.path(), &keys(), cfg).unwrap();
    let evs = events(3);
    for e in &evs {
        r.append(e).unwrap();
    }
    r.preserve(0).unwrap();
    for _ in 0..8 {
        for e in &evs {
            r.append(e).unwrap();
        }
    }
    r.seal().unwrap();
    let segs = r.list_segments().unwrap();
    assert!(segs.iter().any(|s| s.index == 0 && s.preserved), "preserved evidence must survive");
    assert!(segs.len() < 9, "old unpreserved segments must have been pruned");
    assert!(r.verify().is_ok(), "gaps from retention must not break verification");
}

/// Pacotes do grafo de dependências (build + normais) de `pkg` no alvo Windows.
/// O `Cargo.lock` é multiplataforma e lista crates de outros sistemas, então não serve aqui.
fn windows_closure(pkg: &str) -> std::collections::BTreeSet<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args([
            "tree",
            "-p",
            pkg,
            "--target",
            "x86_64-pc-windows-msvc",
            "--prefix",
            "none",
            "-e",
            "normal,build",
            "--offline",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("run cargo tree");
    assert!(out.status.success(), "cargo tree failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_whitespace().next()).map(str::to_owned).collect()
}

const HTTP_TLS_CLIENTS: [&str; 9] = ["reqwest", "hyper", "ureq", "curl", "isahc", "surf", "native-tls", "openssl", "rustls"];
const SOCKET_STACKS: [&str; 4] = ["tokio", "mio", "socket2", "h2"];

// O núcleo não depende de nenhuma biblioteca de rede ou de sockets.
#[test]
fn core_crates_have_no_network_or_socket_dependencies() {
    for pkg in ["bb-core", "bb-recorder", "bb-collector", "bb-store", "bb-engine", "bb-tray"] {
        let deps = windows_closure(pkg);
        assert!(deps.contains(pkg), "cargo tree did not report {pkg}");
        for banned in HTTP_TLS_CLIENTS.iter().chain(SOCKET_STACKS.iter()) {
            assert!(!deps.contains(*banned), "{pkg} depends on network/socket crate: {banned}");
        }
    }
}

// O shell Tauri linka tokio/mio (runtime assíncrono do Tauri), mas nenhum cliente HTTP/TLS.
#[test]
fn desktop_shell_has_no_http_or_tls_client() {
    let deps = windows_closure("bb-app");
    assert!(deps.contains("bb-app"));
    for banned in HTTP_TLS_CLIENTS {
        assert!(!deps.contains(banned), "desktop app depends on HTTP/TLS client: {banned}");
    }
}

#[cfg(windows)]
#[test]
fn dpapi_key_is_stable_and_not_stored_in_clear() {
    use bb_recorder::{DpapiKeyStore, KeyProvider};
    let dir = tempfile::tempdir().unwrap();
    let store = DpapiKeyStore::new(dir.path().join("key.bin"));
    let k1 = store.key().unwrap();
    let k2 = DpapiKeyStore::new(dir.path().join("key.bin")).key().unwrap();
    assert_eq!(k1, k2);
    let on_disk = fs::read(dir.path().join("key.bin")).unwrap();
    assert!(!on_disk.windows(32).any(|w| w == k1));
}
