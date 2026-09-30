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

/// Apaga a subchave de teste inteira (mesmo se o teste falhar no meio).
struct KeyCleanup(String);
impl Drop for KeyCleanup {
    fn drop(&mut self) {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};
        let path: Vec<u16> = self.0.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: cadeia UTF-16 terminada em zero que vive durante a chamada.
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr())) };
    }
}

/// Regressão achada pelo CI do GitHub: num Windows em que a chave `Run` ainda não existe, ligar o início
/// automático falhava com "arquivo não encontrado". Agora a chave é criada quando falta.
#[test]
fn enable_creates_the_run_key_when_it_does_not_exist_yet() {
    let root = format!("Software\\BlackBoxTest-{}", std::process::id());
    let _cleanup = KeyCleanup(root.clone());
    let e = StartupEntry::in_key(&format!("{root}\\Run"), "DeveloperBlackBoxTest-missing-key");

    // Com a chave ausente: nada registrado, e desligar não é erro.
    assert!(!e.is_enabled());
    assert_eq!(e.command(), None);
    e.disable().expect("disable on a missing key is not an error");

    e.enable(&fake_exe(), "--minimized").expect("enable must create the missing key");
    assert_eq!(e.command().unwrap(), format!("\"{}\" --minimized", fake_exe().display()));
    e.disable().unwrap();
    assert!(!e.is_enabled());
}
