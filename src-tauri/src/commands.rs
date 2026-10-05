//! Comandos do dashboard: uma camada fina sobre `bb-query` e o `Engine`.
//! Nenhum comando devolve conteúdo além do que o modelo de eventos permite.
//!
//! Erros são sempre CÓDIGOS estáveis (ex. `auth.not_protected`), nunca texto de um idioma nem
//! detalhes internos (que poderiam citar caminhos): a interface os traduz.

use std::sync::Arc;

use bb_core::ExclusionSet;
use bb_engine::{EngineError, HealthSourceId, PartialExclusion, Settings, SourceState};
use bb_query::{ActivityFilter, ActivityRow, IncidentDetail, IncidentDto, Overview, ProcessRow, SegmentDto};
use bb_recorder::RecorderError;
use bb_store::InvestigationState;
use bb_tray::Lang;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{refresh, utc_ms, Runtime, WinEngine, LANGUAGE_SETTING};

const MAX_NOTE_CHARS: usize = 2000;

const NO_STORE: &str = "store.unavailable";

fn engine_err(e: EngineError) -> String {
    e.code()
}

fn store_err<E>(_: E) -> String {
    "store.error".into()
}

fn with_engine<T>(app: &AppHandle, f: impl FnOnce(&mut WinEngine) -> Result<T, String>) -> Result<T, String> {
    let rt = app.state::<Arc<Runtime>>();
    let mut e = rt.engine.lock().map_err(|_| "internal".to_string())?;
    f(&mut e)
}

// ---- idioma ----

/// Versão instalada do app (a mesma do Cargo.toml do workspace).
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Idioma atual da interface ("en" ou "pt-BR").
#[tauri::command]
pub fn get_language(app: AppHandle) -> String {
    app.state::<Arc<Runtime>>().lang().code().to_string()
}

/// Troca o idioma na hora (painel e bandeja), salva a escolha (cifrada) e registra a mudança no
/// histórico de configuração (só "language: changed", sem valor).
#[tauri::command]
pub fn set_language(app: AppHandle, language: String) -> Result<String, String> {
    let lang = Lang::parse(&language).ok_or("language.invalid")?;
    let changed = {
        let rt = app.state::<Arc<Runtime>>();
        let mut cur = rt.lang.lock().map_err(|_| "internal".to_string())?;
        let changed = *cur != lang;
        *cur = lang;
        changed
    };
    if changed {
        with_engine(&app, |e| {
            if let Some(store) = e.store() {
                store.set_setting(LANGUAGE_SETTING, lang.code()).map_err(store_err)?;
                let _ = store.log_config_change(utc_ms(), "language", "changed");
            }
            Ok(())
        })?;
    }
    // Retraduz o menu e o tooltip da bandeja e emite o estado já no novo idioma.
    refresh(&app);
    Ok(lang.code().to_string())
}

// ---- painel ----

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
        let store = e.store().ok_or(NO_STORE)?;
        let all = store.list_incidents().map_err(store_err)?;
        Ok(all.iter().map(IncidentDto::from).collect())
    })
}

#[tauri::command]
pub fn get_incident(app: AppHandle, id: i64) -> Result<Option<IncidentDetail>, String> {
    with_engine(&app, |e| {
        let store = e.store().ok_or(NO_STORE)?;
        Ok(bb_query::incident_detail(e.recorder(), store, id))
    })
}

#[tauri::command]
pub fn set_incident_state(app: AppHandle, id: i64, state: String) -> Result<(), String> {
    let s = InvestigationState::parse(&state).ok_or("incident.bad_state")?;
    with_engine(&app, |e| e.store().ok_or(NO_STORE)?.set_investigation_state(id, s).map_err(store_err))
}

#[tauri::command]
pub fn add_incident_note(app: AppHandle, id: i64, text: String) -> Result<(), String> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err("note.empty".into());
    }
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err("note.too_long".into());
    }
    with_engine(&app, |e| e.store().ok_or(NO_STORE)?.add_note(id, utc_ms(), &text).map(|_| ()).map_err(store_err))
}

#[tauri::command]
pub fn delete_incident(app: AppHandle, id: i64) -> Result<(), String> {
    with_engine(&app, |e| e.delete_incident(id).map_err(engine_err))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDto {
    path: String,
    events: usize,
    dropped: usize,
}

/// Exporta as evidências do incidente para `%LOCALAPPDATA%\DeveloperBlackBox\exports`,
/// aplicando de novo as regras de privacidade de agora. O arquivo é neutro quanto a idioma.
#[tauri::command]
pub fn export_incident(app: AppHandle, id: i64) -> Result<ExportDto, String> {
    let out = crate::data_dir().map_err(|_| "internal".to_string())?.join("exports");
    let mono = app.state::<Arc<Runtime>>().mono_ms();
    with_engine(&app, |e| {
        let r = e.export_incident(id, &out, mono, utc_ms()).map_err(engine_err)?;
        Ok(ExportDto { path: r.path.display().to_string(), events: r.events, dropped: r.dropped })
    })
}

#[tauri::command]
pub fn capture_incident(app: AppHandle) -> Result<i64, String> {
    with_engine(&app, |e| e.capture_manual(utc_ms()).map_err(engine_err))
}

/// Exclusão parcial na interface: do programa `exe`, os tipos em `kinds` (códigos `lifecycle`, `metrics`,
/// `crashes`) NÃO são gravados.
#[derive(Serialize, Deserialize)]
pub struct PartialExclusionDto {
    exe: String,
    kinds: Vec<String>,
}

/// Quantas vezes uma regra de exclusão deixou algo de fora desde que o app abriu. Só o programa da regra e um número.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OmittedDto {
    exe: String,
    count: u64,
}

#[tauri::command]
pub fn get_omitted_counts(app: AppHandle) -> Result<Vec<OmittedDto>, String> {
    with_engine(&app, |e| Ok(e.omitted_counts().into_iter().map(|(exe, count)| OmittedDto { exe, count }).collect()))
}

/// Uma linha da aba "Saúde do sistema": a fonte e o estado dela agora. Só enumerações fixas.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSourceDto {
    source: &'static str,
    state: &'static str,
}

#[tauri::command]
pub fn get_health_status(app: AppHandle) -> Result<Vec<HealthSourceDto>, String> {
    let mono = app.state::<Arc<Runtime>>().mono_ms();
    with_engine(&app, |e| {
        Ok(e.health_sources(mono)
            .into_iter()
            .map(|(id, st)| HealthSourceDto {
                source: match id {
                    HealthSourceId::EventLog => "eventLog",
                    HealthSourceId::Inventory => "inventory",
                    HealthSourceId::Power => "power",
                    HealthSourceId::Telemetry => "telemetry",
                },
                state: match st {
                    SourceState::Ok => "ok",
                    SourceState::Attention => "attention",
                    SourceState::Unavailable => "unavailable",
                    SourceState::Waiting => "waiting",
                    SourceState::Paused => "paused",
                    SourceState::Off => "off",
                },
            })
            .collect())
    })
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    protected_apps: Vec<String>,
    excluded_apps: Vec<String>,
    // Sem `#[serde(default)]` de propósito: um cliente que esqueça este campo é recusado em vez de apagar
    // em silêncio as exclusões parciais (o que deixaria programas menos excluídos do que o usuário quer).
    partial_exclusions: Vec<PartialExclusionDto>,
    // Idem: sem `default`, para um cliente que esqueça este campo ser recusado em vez de desligar em silêncio a opção
    // de excluir também os processos filhos.
    excluded_trees: Vec<String>,
    stability_window_ms: u64,
    auto_start: bool,
    retention_max_mb: u64,
    retention_max_hours: u64,
    // Sem `default`, como os campos acima: um cliente que esqueça este campo é recusado em vez de ligar ou desligar a
    // telemetria em silêncio.
    telemetry_enabled: bool,
    // Idem para os três interruptores das fontes de saúde: sem `default`, um cliente que os esqueça é recusado.
    health_log_enabled: bool,
    inventory_enabled: bool,
    power_enabled: bool,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            protected_apps: s.protected_apps.clone(),
            excluded_apps: s.excluded_apps.clone(),
            partial_exclusions: s
                .partial_exclusions
                .iter()
                .map(|r| PartialExclusionDto {
                    exe: r.exe.clone(),
                    kinds: r.excluded.kinds().iter().map(|k| k.code().to_owned()).collect(),
                })
                .collect(),
            excluded_trees: s.excluded_trees.clone(),
            stability_window_ms: s.stability_window_ms,
            auto_start: s.auto_start,
            retention_max_mb: s.retention_max_mb,
            retention_max_hours: s.retention_max_hours,
            telemetry_enabled: s.telemetry_enabled,
            health_log_enabled: s.health_log_enabled,
            inventory_enabled: s.inventory_enabled,
            power_enabled: s.power_enabled,
        }
    }
}

impl From<SettingsDto> for Settings {
    fn from(d: SettingsDto) -> Self {
        Settings {
            protected_apps: d.protected_apps,
            excluded_apps: d.excluded_apps,
            // Códigos desconhecidos ou lista vazia viram exclusão TOTAL (falha fechada): nunca menos do que o pedido.
            partial_exclusions: d
                .partial_exclusions
                .into_iter()
                .map(|r| PartialExclusion { exe: r.exe, excluded: ExclusionSet::from_codes(&r.kinds.join(",")) })
                .collect(),
            excluded_trees: d.excluded_trees,
            stability_window_ms: d.stability_window_ms,
            auto_start: d.auto_start,
            retention_max_mb: d.retention_max_mb,
            retention_max_hours: d.retention_max_hours,
            telemetry_enabled: d.telemetry_enabled,
            health_log_enabled: d.health_log_enabled,
            inventory_enabled: d.inventory_enabled,
            power_enabled: d.power_enabled,
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
        e.apply_settings(settings.into(), utc_ms()).map_err(engine_err)?;
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
    fn startup_err<E>(_: E) -> String {
        "startup.error".into()
    }
    if enabled {
        let exe = std::env::current_exe().map_err(startup_err)?;
        entry.enable(&exe, crate::MINIMIZED_ARG).map_err(startup_err)?;
    } else {
        entry.disable().map_err(startup_err)?;
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
        e.authorize_app(&exe, minutes, allow_metrics, allow_crashes, mono, utc_ms()).map_err(engine_err)
    })?;
    refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn revoke_authorization(app: AppHandle, exe: String) -> Result<bool, String> {
    let existed = with_engine(&app, |e| e.revoke_authorization(&exe, utc_ms()).map_err(engine_err))?;
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
        let store = e.store().ok_or(NO_STORE)?;
        let h = store.config_history(50).map_err(store_err)?;
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
            .map_err(|_| "recorder.error".to_string())?
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
    /// Código do problema (a interface o traduz), nunca o texto interno do erro.
    error: Option<String>,
}

/// Código traduzível para uma falha de integridade. Não expõe detalhes internos.
fn verify_code(err: &RecorderError) -> &'static str {
    match err {
        RecorderError::Crypto => "verify.auth_failed",
        RecorderError::Corrupt("hash chain broken") => "verify.chain_broken",
        RecorderError::Corrupt("missing segment") => "verify.missing_segment",
        RecorderError::Corrupt(_) => "verify.corrupt",
        _ => "verify.io",
    }
}

#[tauri::command]
pub fn verify_integrity(app: AppHandle) -> Result<VerifyDto, String> {
    with_engine(&app, |e| {
        Ok(match e.recorder().verify() {
            Ok(r) => VerifyDto { ok: true, segments: r.segments, events: r.events, error: None },
            Err(err) => VerifyDto { ok: false, segments: 0, events: 0, error: Some(verify_code(&err).into()) },
        })
    })
}

#[tauri::command]
pub fn delete_activity(app: AppHandle, include_preserved: bool) -> Result<usize, String> {
    with_engine(&app, |e| e.delete_activity(include_preserved).map_err(engine_err))
}
