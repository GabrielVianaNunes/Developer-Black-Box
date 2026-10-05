//! Metadados locais em SQLite: incidentes e seus manifestos de evidência.
//!
//! Sem campos de texto livre: o resumo de um incidente é gerado a partir de
//! enums e números. Os dados de atividade em si ficam nos segmentos cifrados.

mod fieldcrypt;

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use fieldcrypt::{is_encrypted, FieldCipher};

macro_rules! text_enum {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $s),+ }
            }
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$variant),)+ _ => None }
            }
        }
    };
}

text_enum!(IncidentKind {
    Manual => "manual",
    CpuSustained => "cpu_sustained",
    MemoryHigh => "memory_high",
    UnexpectedExit => "unexpected_exit",
    AppHang => "app_hang",
    UnexpectedShutdown => "unexpected_shutdown",
    BlueScreen => "blue_screen",
    HardwareError => "hardware_error",
    Throttling => "throttling",
});
text_enum!(Severity { Info => "info", Warning => "warning", Critical => "critical" });
text_enum!(InvestigationState {
    New => "new",
    Investigating => "investigating",
    Resolved => "resolved",
    Dismissed => "dismissed",
});
text_enum!(CaptureState { Capturing => "capturing", Preserved => "preserved" });

#[derive(Clone, Debug)]
pub struct NewIncident {
    pub kind: IncidentKind,
    pub severity: Severity,
    pub created_utc_ms: i64,
    /// Só o nome do executável, e apenas quando o Guard permitiu o evento de origem.
    pub exe_name: Option<String>,
    pub summary: String,
    pub post_until_utc_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incident {
    pub id: i64,
    pub kind: IncidentKind,
    pub severity: Severity,
    pub created_utc_ms: i64,
    pub exe_name: Option<String>,
    pub summary: String,
    pub state: InvestigationState,
    pub capture: CaptureState,
    pub post_until_utc_ms: i64,
    /// Manifesto: índices dos segmentos de evidência.
    pub segments: Vec<u64>,
}

#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "store error: {}", self.0)
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Campos cifrados quando o `Store` é aberto com chave: nome do executável do incidente,
/// texto das anotações e valores das configurações (listas de apps). O resto são enums,
/// números e horários.
pub struct Store {
    conn: Connection,
    cipher: Option<FieldCipher>,
}

/// Anotação escrita pelo próprio usuário sobre um incidente. É texto livre do usuário
/// (nunca preenchida automaticamente); cifrada quando o `Store` tem chave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: i64,
    pub incident_id: i64,
    pub created_utc_ms: i64,
    pub text: String,
}

/// Registro mínimo de mudança de configuração: só qual chave e o tipo de mudança,
/// nunca o valor (que poderia ser o nome de um app).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigChange {
    pub id: i64,
    pub at_utc_ms: i64,
    pub key: String,
    pub change: String,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS config_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at_utc_ms INTEGER NOT NULL,
    key TEXT NOT NULL,
    change TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    incident_id INTEGER NOT NULL,
    created_utc_ms INTEGER NOT NULL,
    text TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS incidents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    created_utc_ms INTEGER NOT NULL,
    exe_name TEXT,
    summary TEXT NOT NULL,
    state TEXT NOT NULL,
    capture TEXT NOT NULL,
    post_until_utc_ms INTEGER NOT NULL,
    segments TEXT NOT NULL DEFAULT ''
);";

fn row_to_incident(r: &Row<'_>) -> rusqlite::Result<Incident> {
    let bad = |c: usize| rusqlite::Error::InvalidColumnType(c, "enum".into(), rusqlite::types::Type::Text);
    let segs: String = r.get(9)?;
    Ok(Incident {
        id: r.get(0)?,
        kind: IncidentKind::parse(&r.get::<_, String>(1)?).ok_or_else(|| bad(1))?,
        severity: Severity::parse(&r.get::<_, String>(2)?).ok_or_else(|| bad(2))?,
        created_utc_ms: r.get(3)?,
        exe_name: r.get(4)?,
        summary: r.get(5)?,
        state: InvestigationState::parse(&r.get::<_, String>(6)?).ok_or_else(|| bad(6))?,
        capture: CaptureState::parse(&r.get::<_, String>(7)?).ok_or_else(|| bad(7))?,
        post_until_utc_ms: r.get(8)?,
        segments: segs.split(',').filter_map(|s| s.parse().ok()).collect(),
    })
}

const COLS: &str = "id, kind, severity, created_utc_ms, exe_name, summary, state, capture, post_until_utc_ms, segments";

impl Store {
    /// Sem cifra de campos (dados de teste e sem chave disponível).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?, None)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    /// Com cifra dos campos sensíveis. Dados antigos em texto claro são migrados na hora.
    /// Se a chave estiver errada, os campos cifrados não abrem (nunca voltam como texto claro).
    pub fn open_encrypted(path: impl AsRef<Path>, master_key: &[u8; 32]) -> Result<Self> {
        Self::init(Connection::open(path)?, Some(FieldCipher::new(master_key)))
    }

    pub fn open_in_memory_encrypted(master_key: &[u8; 32]) -> Result<Self> {
        Self::init(Connection::open_in_memory()?, Some(FieldCipher::new(master_key)))
    }

    fn init(conn: Connection, cipher: Option<FieldCipher>) -> Result<Self> {
        // Conteúdo apagado (incidentes, anotações) é sobrescrito no arquivo, não só desvinculado.
        conn.execute_batch("PRAGMA secure_delete = ON;")?;
        conn.execute_batch(SCHEMA)?;
        let s = Self { conn, cipher };
        s.migrate_plaintext()?;
        Ok(s)
    }

    fn enc(&self, ctx: &str, v: &str) -> String {
        match &self.cipher {
            Some(c) => c.encrypt(ctx, v),
            None => v.to_owned(),
        }
    }

    fn dec(&self, ctx: &str, stored: String) -> Result<String> {
        match &self.cipher {
            Some(c) => {
                c.decrypt(ctx, &stored).ok_or_else(|| StoreError("cannot decrypt a stored field (wrong key or tampered data)".into()))
            }
            None if is_encrypted(&stored) => Err(StoreError("stored data is encrypted but no key was provided".into())),
            None => Ok(stored),
        }
    }

    fn decode_incident(&self, mut i: Incident) -> Result<Incident> {
        if let Some(e) = i.exe_name.take() {
            i.exe_name = Some(self.dec("incidents.exe_name", e)?);
        }
        Ok(i)
    }

    /// Cifra o que ainda estiver em texto claro e limpa páginas livres do arquivo (`VACUUM`).
    fn migrate_plaintext(&self) -> Result<()> {
        let Some(c) = &self.cipher else { return Ok(()) };
        let mut changed = false;

        let rows: Vec<(i64, String)> = self
            .conn
            .prepare("SELECT id, exe_name FROM incidents WHERE exe_name IS NOT NULL")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, v) in rows.into_iter().filter(|(_, v)| !is_encrypted(v)) {
            let e = c.encrypt("incidents.exe_name", &v);
            self.conn.execute("UPDATE incidents SET exe_name = ?1 WHERE id = ?2", params![e, id])?;
            changed = true;
        }

        let rows: Vec<(i64, String)> = self
            .conn
            .prepare("SELECT id, text FROM notes")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, v) in rows.into_iter().filter(|(_, v)| !is_encrypted(v)) {
            let e = c.encrypt("notes.text", &v);
            self.conn.execute("UPDATE notes SET text = ?1 WHERE id = ?2", params![e, id])?;
            changed = true;
        }

        let rows: Vec<(String, String)> = self
            .conn
            .prepare("SELECT key, value FROM settings")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (k, v) in rows.into_iter().filter(|(_, v)| !is_encrypted(v)) {
            let e = c.encrypt(&format!("settings.{k}"), &v);
            self.conn.execute("UPDATE settings SET value = ?1 WHERE key = ?2", params![e, k])?;
            changed = true;
        }

        if changed {
            self.conn.execute_batch("VACUUM;")?;
        }
        Ok(())
    }

    pub fn create_incident(&self, n: &NewIncident) -> Result<i64> {
        let exe = n.exe_name.as_deref().map(|e| self.enc("incidents.exe_name", e));
        self.conn.execute(
            "INSERT INTO incidents (kind, severity, created_utc_ms, exe_name, summary, state, capture, post_until_utc_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, 'new', 'capturing', ?6)",
            params![n.kind.as_str(), n.severity.as_str(), n.created_utc_ms, exe, n.summary, n.post_until_utc_ms],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_incident(&self, id: i64) -> Result<Option<Incident>> {
        let raw = self.conn.query_row(&format!("SELECT {COLS} FROM incidents WHERE id = ?1"), [id], row_to_incident).optional()?;
        raw.map(|i| self.decode_incident(i)).transpose()
    }

    pub fn list_incidents(&self) -> Result<Vec<Incident>> {
        let mut st = self.conn.prepare(&format!("SELECT {COLS} FROM incidents ORDER BY created_utc_ms DESC, id DESC"))?;
        let raw = st.query_map([], row_to_incident)?.collect::<rusqlite::Result<Vec<_>>>()?;
        raw.into_iter().map(|i| self.decode_incident(i)).collect()
    }

    pub fn pending_captures(&self) -> Result<Vec<Incident>> {
        let mut st = self.conn.prepare(&format!("SELECT {COLS} FROM incidents WHERE capture = 'capturing' ORDER BY id"))?;
        let raw = st.query_map([], row_to_incident)?.collect::<rusqlite::Result<Vec<_>>>()?;
        raw.into_iter().map(|i| self.decode_incident(i)).collect()
    }

    pub fn set_segments(&self, id: i64, segments: &[u64]) -> Result<()> {
        let s: Vec<String> = segments.iter().map(u64::to_string).collect();
        self.conn.execute("UPDATE incidents SET segments = ?1 WHERE id = ?2", params![s.join(","), id])?;
        Ok(())
    }

    pub fn set_capture_state(&self, id: i64, c: CaptureState) -> Result<()> {
        self.conn.execute("UPDATE incidents SET capture = ?1 WHERE id = ?2", params![c.as_str(), id])?;
        Ok(())
    }

    pub fn set_investigation_state(&self, id: i64, s: InvestigationState) -> Result<()> {
        self.conn.execute("UPDATE incidents SET state = ?1 WHERE id = ?2", params![s.as_str(), id])?;
        Ok(())
    }

    /// Remove o incidente e suas anotações.
    pub fn delete_incident(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM notes WHERE incident_id = ?1", [id])?;
        self.conn.execute("DELETE FROM incidents WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn add_note(&self, incident_id: i64, created_utc_ms: i64, text: &str) -> Result<i64> {
        let stored = self.enc("notes.text", text);
        self.conn.execute(
            "INSERT INTO notes (incident_id, created_utc_ms, text) VALUES (?1, ?2, ?3)",
            params![incident_id, created_utc_ms, stored],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_notes(&self, incident_id: i64) -> Result<Vec<Note>> {
        let mut st = self.conn.prepare("SELECT id, incident_id, created_utc_ms, text FROM notes WHERE incident_id = ?1 ORDER BY id")?;
        let raw = st
            .query_map([incident_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        raw.into_iter()
            .map(|(id, incident_id, created_utc_ms, text)| {
                Ok(Note { id, incident_id, created_utc_ms, text: self.dec("notes.text", text)? })
            })
            .collect()
    }

    pub fn delete_note(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM notes WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let raw: Option<String> = self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        raw.map(|v| self.dec(&format!("settings.{key}"), v)).transpose()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let stored = self.enc(&format!("settings.{key}"), value);
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, stored],
        )?;
        Ok(())
    }

    pub fn log_config_change(&self, at_utc_ms: i64, key: &str, change: &str) -> Result<()> {
        self.conn.execute("INSERT INTO config_history (at_utc_ms, key, change) VALUES (?1, ?2, ?3)", params![at_utc_ms, key, change])?;
        Ok(())
    }

    /// Mais recentes primeiro.
    pub fn config_history(&self, limit: usize) -> Result<Vec<ConfigChange>> {
        let mut st = self.conn.prepare("SELECT id, at_utc_ms, key, change FROM config_history ORDER BY id DESC LIMIT ?1")?;
        let rows =
            st.query_map([limit as i64], |r| Ok(ConfigChange { id: r.get(0)?, at_utc_ms: r.get(1)?, key: r.get(2)?, change: r.get(3)? }))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_inc(t: i64) -> NewIncident {
        NewIncident {
            kind: IncidentKind::Manual,
            severity: Severity::Info,
            created_utc_ms: t,
            exe_name: None,
            summary: "manual capture".into(),
            post_until_utc_ms: t + 100,
        }
    }

    #[test]
    fn create_get_update_delete() {
        let s = Store::open_in_memory().unwrap();
        let id = s.create_incident(&new_inc(10)).unwrap();
        let i = s.get_incident(id).unwrap().unwrap();
        assert_eq!((i.state, i.capture), (InvestigationState::New, CaptureState::Capturing));
        s.set_segments(id, &[3, 4]).unwrap();
        s.set_capture_state(id, CaptureState::Preserved).unwrap();
        s.set_investigation_state(id, InvestigationState::Investigating).unwrap();
        let i = s.get_incident(id).unwrap().unwrap();
        assert_eq!(i.segments, vec![3, 4]);
        assert_eq!(i.capture, CaptureState::Preserved);
        assert_eq!(i.state, InvestigationState::Investigating);
        s.delete_incident(id).unwrap();
        assert!(s.get_incident(id).unwrap().is_none());
    }

    #[test]
    fn list_is_newest_first_and_pending_only_lists_capturing() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_incident(&new_inc(10)).unwrap();
        let b = s.create_incident(&new_inc(20)).unwrap();
        s.set_capture_state(a, CaptureState::Preserved).unwrap();
        let ids: Vec<i64> = s.list_incidents().unwrap().iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![b, a]);
        let pending: Vec<i64> = s.pending_captures().unwrap().iter().map(|i| i.id).collect();
        assert_eq!(pending, vec![b]);
    }

    #[test]
    fn notes_belong_to_an_incident_and_go_away_with_it() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_incident(&new_inc(1)).unwrap();
        let b = s.create_incident(&new_inc(2)).unwrap();
        s.add_note(a, 10, "first").unwrap();
        s.add_note(a, 11, "second").unwrap();
        s.add_note(b, 12, "other").unwrap();
        let texts: Vec<String> = s.list_notes(a).unwrap().into_iter().map(|n| n.text).collect();
        assert_eq!(texts, vec!["first", "second"]);
        s.delete_incident(a).unwrap();
        assert!(s.list_notes(a).unwrap().is_empty());
        assert_eq!(s.list_notes(b).unwrap().len(), 1);
    }

    #[test]
    fn settings_upsert_and_default_to_none() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_setting("k").unwrap(), None);
        s.set_setting("k", "1").unwrap();
        s.set_setting("k", "2").unwrap();
        assert_eq!(s.get_setting("k").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn config_history_is_newest_first_and_limited() {
        let s = Store::open_in_memory().unwrap();
        for i in 0..5 {
            s.log_config_change(i, "excluded_apps", "changed").unwrap();
        }
        let h = s.config_history(3).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].at_utc_ms, 4);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("meta.db");
        let id = Store::open(&p).unwrap().create_incident(&new_inc(5)).unwrap();
        assert!(Store::open(&p).unwrap().get_incident(id).unwrap().is_some());
    }
}
