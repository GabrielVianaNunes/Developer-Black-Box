//! Comandos do dashboard: uma camada fina sobre `bb-query` e o `Engine`.
//! Nenhum comando devolve conteúdo além do que o modelo de eventos permite.

use std::sync::Arc;

use bb_engine::Settings;
use bb_query::{ActivityFilter, ActivityRow, IncidentDetail, IncidentDto, Overview, ProcessRow, SegmentDto};
use bb_store::InvestigationState;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{refresh, utc_ms, Runtime, WinEngine};

const MAX_NOTE_CHARS: usize = 2000;

fn with_engine<T>(app: &AppHandle, f: impl FnOnce(&mut WinEngine) -> Result<T, String>) -> Result<T, String> {
    let rt = app.state::<Arc<Runtime>>();
    let mut e = rt.engine.lock().map_err(|_| "internal error".to_string())?;
    f(&mut e)
}

#[tauri::command]
pub fn get_overview(app: AppHandle) -> Result<Overview, String> {
    with_engine(&app, |e| Ok(bb_query::overview(e.recorder(), e.store(), utc_ms())))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityFilterDto {
    kinds: Vec<String>,
    text: String,
    from_utc_ms: Option<i64>,
    to_utc_ms: Option<i64>,
    limit: usize,
}

#[tauri::command]
pub fn get_activity(app: AppHandle, filter: ActivityFilterDto) -> Result<Vec<ActivityRow>, String> {
    let f = ActivityFilter {
        kinds: filter.kinds,
        text: filter.text,
        from_utc_ms: filter.from_utc_ms,
        to_utc_ms: filter.to_utc_ms,
        limit: if filter.limit == 0 { 200 } else { filter.limit },
    };
    with_engine(&app, |e| Ok(bb_query::activity(e.recorder(), &f)))
}

#[tauri::command]
pub fn get_processes(app: AppHandle) -> Result<Vec<ProcessRow>, String> {
    with_engine(&app, |e| Ok(bb_query::processes(e.recorder(), 300)))
}

#[tauri::command]
pub fn list_incidents(app: AppHandle) -> Result<Vec<IncidentDto>, String> {
    with_engine(&app, |e| {
        let store = e.store().ok_or("incident storage unavailable")?;
        let all = store.list_incidents().map_err(|e| e.to_string())?;
        Ok(all.iter().map(IncidentDto::from).collect())
    })
}

#[tauri::command]
pub fn get_incident(app: AppHandle, id: i64) -> Result<Option<IncidentDetail>, String> {
    with_engine(&app, |e| {
        let store = e.store().ok_or("incident storage unavailable")?;
        Ok(bb_query::incident_detail(e.recorder(), store, id))
    })
}

#[tauri::command]
pub fn set_incident_state(app: AppHandle, id: i64, state: String) -> Result<(), String> {
    let s = InvestigationState::parse(&state).ok_or("invalid investigation state")?;
    with_engine(&app, |e| {
        e.store().ok_or("incident storage unavailable")?.set_investigation_state(id, s).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn add_incident_note(app: AppHandle, id: i64, text: String) -> Result<(), String> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err("empty note".into());
    }
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(format!("note too long (max {MAX_NOTE_CHARS} characters)"));
    }
    with_engine(&app, |e| {
        e.store().ok_or("incident storage unavailable")?.add_note(id, utc_ms(), &text).map(|_| ()).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn delete_incident(app: AppHandle, id: i64) -> Result<(), String> {
    with_engine(&app, |e| e.delete_incident(id).map_err(|e| e.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDto {
    path: String,
    events: usize,
    dropped: usize,
}

/// Exporta as evidências do incidente para `%LOCALAPPDATA%\DeveloperBlackBox\exports`,
/// aplicando de novo as regras de privacidade de agora.
#[tauri::command]
pub fn export_incident(app: AppHandle, id: i64) -> Result<ExportDto, String> {
    let out = crate::data_dir()?.join("exports");
    let mono = app.state::<Arc<Runtime>>().mono_ms();
    with_engine(&app, |e| {
        let r = e.export_incident(id, &out, mono, utc_ms()).map_err(|e| e.to_string())?;
        Ok(ExportDto { path: r.path.display().to_string(), events: r.events, dropped: r.dropped })
    })
}

#[tauri::command]
pub fn capture_incident(app: AppHandle) -> Result<i64, String> {
    with_engine(&app, |e| e.capture_manual(utc_ms()).map_err(|e| e.to_string()))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    protected_apps: Vec<String>,
    excluded_apps: Vec<String>,
    stability_window_ms: u64,
    auto_start: bool,
    retention_max_mb: u64,
    retention_max_hours: u64,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            protected_apps: s.protected_apps.clone(),
            excluded_apps: s.excluded_apps.clone(),
            stability_window_ms: s.stability_window_ms,
            auto_start: s.auto_start,
            retention_max_mb: s.retention_max_mb,
            retention_max_hours: s.retention_max_hours,
        }
    }
}

impl From<SettingsDto> for Settings {
    fn from(d: SettingsDto) -> Self {
        Settings {
            protected_apps: d.protected_apps,
            excluded_apps: d.excluded_apps,
            stability_window_ms: d.stability_window_ms,
            auto_start: d.auto_start,
            retention_max_mb: d.retention_max_mb,
            retention_max_hours: d.retention_max_hours,
        }
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<SettingsDto, String> {
    with_engine(&app, |e| Ok(SettingsDto::from(e.settings())))
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: SettingsDto) -> Result<SettingsDto, String> {
    let out = with_engine(&app, |e| {
        e.apply_settings(settings.into(), utc_ms()).map_err(|e| e.to_string())?;
        Ok(SettingsDto::from(e.settings()))
    })?;
    // A regra nova já vale; a bandeja e a janela refletem o estado resultante.
    refresh(&app);
    Ok(out)
}

/// Se o app abre com o Windows (a fonte da verdade é a chave Run do registro do usuário).
#[tauri::command]
pub fn get_launch_at_login() -> bool {
    bb_collector::startup::StartupEntry::app().is_enabled()
}

/// Liga ou desliga a abertura com o Windows. Abre escondido na bandeja e NÃO inicia a gravação.
#[tauri::command]
pub fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let entry = bb_collector::startup::StartupEntry::app();
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        entry.enable(&exe, crate::MINIMIZED_ARG).map_err(|e| e.to_string())?;
    } else {
        entry.disable().map_err(|e| e.to_string())?;
    }
    with_engine(&app, |e| {
        if let Some(store) = e.store() {
            let _ = store.log_config_change(utc_ms(), "launch_at_login", "changed");
        }
        Ok(())
    })?;
    Ok(entry.is_enabled())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationDto {
    exe: String,
    remaining_ms: u64,
    allow_metrics: bool,
    allow_crashes: bool,
}

#[tauri::command]
pub fn list_authorizations(app: AppHandle) -> Result<Vec<AuthorizationDto>, String> {
    let mono = app.state::<Arc<Runtime>>().mono_ms();
    with_engine(&app, |e| {
        Ok(e.authorizations(mono)
            .into_iter()
            .map(|a| AuthorizationDto {
                exe: a.exe,
                remaining_ms: a.remaining_ms,
                allow_metrics: a.allow_metrics,
                allow_crashes: a.allow_crashes,
            })
            .collect())
    })
}

#[tauri::command]
pub fn authorize_app(
    app: AppHandle,
    exe: String,
    minutes: u64,
    allow_metrics: bool,
    allow_crashes: bool,
) -> Result<(), String> {
    let mono = app.state::<Arc<Runtime>>().mono_ms();
    with_engine(&app, |e| {
        e.authorize_app(&exe, minutes, allow_metrics, allow_crashes, mono, utc_ms()).map_err(|e| e.to_string())
    })?;
    refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn revoke_authorization(app: AppHandle, exe: String) -> Result<bool, String> {
    let existed = with_engine(&app, |e| e.revoke_authorization(&exe, utc_ms()).map_err(|e| e.to_string()))?;
    refresh(&app);
    Ok(existed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChangeDto {
    at_utc_ms: i64,
    key: String,
    change: String,
}

#[tauri::command]
pub fn get_config_history(app: AppHandle) -> Result<Vec<ConfigChangeDto>, String> {
    with_engine(&app, |e| {
        let store = e.store().ok_or("storage unavailable")?;
        let h = store.config_history(50).map_err(|e| e.to_string())?;
        Ok(h.into_iter().map(|c| ConfigChangeDto { at_utc_ms: c.at_utc_ms, key: c.key, change: c.change }).collect())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageDto {
    storage_bytes: u64,
    max_total_bytes: u64,
    max_age_hours: u64,
    segments: Vec<SegmentDto>,
}

#[tauri::command]
pub fn get_storage(app: AppHandle) -> Result<StorageDto, String> {
    with_engine(&app, |e| {
        let rec = e.recorder();
        let segments = rec
            .list_segments()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| SegmentDto { index: s.index, size: s.size, preserved: s.preserved })
            .collect();
        Ok(StorageDto {
            storage_bytes: rec.storage_bytes(),
            max_total_bytes: rec.config().max_total_bytes,
            max_age_hours: rec.config().max_age.map_or(0, |d| d.as_secs() / 3600),
            segments,
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyDto {
    ok: bool,
    segments: usize,
    events: u64,
    error: Option<String>,
}

#[tauri::command]
pub fn verify_integrity(app: AppHandle) -> Result<VerifyDto, String> {
    with_engine(&app, |e| {
        Ok(match e.recorder().verify() {
            Ok(r) => VerifyDto { ok: true, segments: r.segments, events: r.events, error: None },
            Err(err) => VerifyDto { ok: false, segments: 0, events: 0, error: Some(err.to_string()) },
        })
    })
}

#[tauri::command]
pub fn delete_activity(app: AppHandle, include_preserved: bool) -> Result<usize, String> {
    with_engine(&app, |e| e.delete_activity(include_preserved).map_err(|e| e.to_string()))
}
