//! Início automático com o Windows, contra o registro REAL do usuário atual.
//! Usa um nome de valor exclusivo de teste (nunca o do app) e sempre o remove.
#![cfg(windows)]

use std::path::PathBuf;

use bb_collector::startup::{StartupEntry, APP_VALUE_NAME};

/// Remove o valor de teste mesmo se o teste falhar no meio.
struct Cleanup(StartupEntry);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.disable();
    }
}

fn test_entry(tag: &str) -> Cleanup {
    let name = format!("DeveloperBlackBoxTest-{}-{tag}", std::process::id());
    assert_ne!(name, APP_VALUE_NAME, "tests must never touch the app's real entry");
    Cleanup(StartupEntry::new(&name))
}

fn fake_exe() -> PathBuf {
    std::env::temp_dir().join("synthetic-app.exe")
}

#[test]
fn enable_writes_the_quoted_command_and_disable_removes_it() {
    let e = test_entry("roundtrip");
    assert!(!e.0.is_enabled());
    e.0.enable(&fake_exe(), "--minimized").unwrap();
    assert!(e.0.is_enabled());
    let cmd = e.0.command().unwrap();
    assert_eq!(cmd, format!("\"{}\" --minimized", fake_exe().display()));
    e.0.disable().unwrap();
    assert!(!e.0.is_enabled());
    assert_eq!(e.0.command(), None);
}

#[test]
fn disable_is_idempotent_and_enable_overwrites() {
    let e = test_entry("idempotent");
    e.0.disable().unwrap();
    e.0.disable().unwrap();
    e.0.enable(&fake_exe(), "--minimized").unwrap();
    e.0.enable(&fake_exe(), "").unwrap();
    assert_eq!(e.0.command().unwrap(), format!("\"{}\"", fake_exe().display()));
}

#[test]
fn paths_that_could_distort_the_command_are_rejected() {
    let e = test_entry("reject");
    assert!(e.0.enable(&PathBuf::from("relative\\app.exe"), "--minimized").is_err(), "relative path");
    assert!(e.0.enable(&PathBuf::from("C:\\bad\"name\\app.exe"), "--minimized").is_err(), "quote in path");
    assert!(!e.0.is_enabled(), "a rejected request must not write anything");
}

#[test]
fn the_apps_own_entry_is_untouched_by_these_tests() {
    // O valor real do app não deve ter sido criado por este arquivo de testes.
    let before = StartupEntry::app().command();
    let e = test_entry("isolation");
    e.0.enable(&fake_exe(), "--minimized").unwrap();
    assert_eq!(StartupEntry::app().command(), before);
}
