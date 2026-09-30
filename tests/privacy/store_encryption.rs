//! Cifra dos campos sensíveis do SQLite e apagamento seguro. Dados sintéticos.

use std::fs;

use bb_store::{IncidentKind, NewIncident, Severity, Store};

const KEY: [u8; 32] = [7; 32];
const EXE: &str = "very-secret-app.exe";
const NOTE: &str = "my confidential investigation note";
const SETTING: &str = "private-tool.exe";

fn incident(exe: Option<&str>) -> NewIncident {
    NewIncident {
        kind: IncidentKind::Manual,
        severity: Severity::Info,
        created_utc_ms: 1,
        exe_name: exe.map(str::to_owned),
        summary: "Captura manual".into(),
        post_until_utc_ms: 2,
    }
}

fn file_contains(path: &std::path::Path, needle: &str) -> bool {
    let bytes = fs::read(path).unwrap();
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn fill(s: &Store) -> i64 {
    let id = s.create_incident(&incident(Some(EXE))).unwrap();
    s.add_note(id, 5, NOTE).unwrap();
    s.set_setting("excluded_apps", SETTING).unwrap();
    id
}

#[test]
fn sensitive_fields_are_not_readable_in_the_database_file_but_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meta.db");
    let id = {
        let s = Store::open_encrypted(&path, &KEY).unwrap();
        fill(&s)
    };
    for secret in [EXE, NOTE, SETTING] {
        assert!(!file_contains(&path, secret), "plaintext found in the database file: {secret}");
    }
    let s = Store::open_encrypted(&path, &KEY).unwrap();
    assert_eq!(s.get_incident(id).unwrap().unwrap().exe_name.as_deref(), Some(EXE));
    assert_eq!(s.list_notes(id).unwrap()[0].text, NOTE);
    assert_eq!(s.get_setting("excluded_apps").unwrap().as_deref(), Some(SETTING));
}

#[test]
fn a_wrong_key_or_no_key_cannot_read_and_never_returns_plaintext_or_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meta.db");
    let id = {
        let s = Store::open_encrypted(&path, &KEY).unwrap();
        fill(&s)
    };
    let other = Store::open_encrypted(&path, &[8; 32]).unwrap();
    assert!(other.get_incident(id).is_err());
    assert!(other.list_notes(id).is_err());
    assert!(other.get_setting("excluded_apps").is_err());

    let no_key = Store::open(&path).unwrap();
    assert!(no_key.get_incident(id).is_err());
    assert!(no_key.list_notes(id).is_err());
    assert!(no_key.get_setting("excluded_apps").is_err());
}

#[test]
fn legacy_plaintext_is_migrated_and_purged_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meta.db");
    let id = {
        let s = Store::open(&path).unwrap();
        fill(&s)
    };
    for secret in [EXE, NOTE, SETTING] {
        assert!(file_contains(&path, secret), "control: the legacy file should hold plaintext ({secret})");
    }
    let s = Store::open_encrypted(&path, &KEY).unwrap();
    assert_eq!(s.get_incident(id).unwrap().unwrap().exe_name.as_deref(), Some(EXE));
    drop(s);
    for secret in [EXE, NOTE, SETTING] {
        assert!(!file_contains(&path, secret), "plaintext survived the migration: {secret}");
    }
}

#[test]
fn deleted_incidents_and_notes_leave_no_readable_trace_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meta.db");
    {
        let s = Store::open(&path).unwrap(); // sem cifra: só o apagamento seguro está em teste
        let id = fill(&s);
        assert!(file_contains(&path, NOTE));
        s.delete_incident(id).unwrap();
    }
    assert!(!file_contains(&path, NOTE), "note text survived deletion");
    assert!(!file_contains(&path, EXE), "executable name survived deletion");
}

#[test]
fn ciphertext_cannot_be_moved_between_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meta.db");
    let id = {
        let s = Store::open_encrypted(&path, &KEY).unwrap();
        fill(&s)
    };
    // um atacante local copia o valor cifrado do nome do app para o texto da anotação
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute("UPDATE notes SET text = (SELECT exe_name FROM incidents WHERE id = ?1)", [id]).unwrap();
    drop(raw);
    let s = Store::open_encrypted(&path, &KEY).unwrap();
    assert!(s.list_notes(id).is_err(), "the column context is authenticated");
}

#[test]
fn config_history_stays_readable_and_holds_no_values() {
    let s = Store::open_in_memory_encrypted(&KEY).unwrap();
    s.log_config_change(1, "excluded_apps", "added").unwrap();
    let h = s.config_history(5).unwrap();
    assert_eq!((h[0].key.as_str(), h[0].change.as_str()), ("excluded_apps", "added"));
}
