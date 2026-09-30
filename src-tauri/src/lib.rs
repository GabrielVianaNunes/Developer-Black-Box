//! Aplicativo desktop: hospeda o Engine, a bandeja e a janela de controle.
//!
//! O ícone, o tooltip e o menu da bandeja são sempre derivados de `Engine::state`,
//! nunca de uma variável de interface. A janela pode estar fechada e a bandeja
//! continua funcionando.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent, Wry};

use bb_collector::{MetricsConfig, WindowsContextSource, WindowsCrashSource, WindowsProcessSource};
use bb_core::{GuardConfig, ReasonCode, RecorderState};
use bb_engine::{Engine, IncidentConfig};
use bb_recorder::{DpapiKeyStore, KeyProvider, Recorder, RecorderConfig};
use bb_store::Store;
use bb_tray::{icon, light_for, menu_model, tooltip, Light};

mod commands;

/// Argumento com que o Windows abre o app no login: janela escondida, só a bandeja.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Se a entrada de início automático aponta para um arquivo que não existe mais (o app foi
/// movido ou reinstalado em outro lugar), reaponta para este executável. Nunca liga o recurso
/// por conta própria e não mexe numa entrada cujo executável ainda existe.
fn heal_startup_entry() {
    use bb_collector::startup::StartupEntry;
    let entry = StartupEntry::app();
    let (Some(cmd), Ok(me)) = (entry.command(), std::env::current_exe()) else { return };
    let registered = cmd.trim_start_matches('"').split('"').next().unwrap_or("");
    if !std::path::Path::new(registered).exists() {
        let _ = entry.enable(&me, MINIMIZED_ARG);
    }
}

type WinEngine = Engine<WindowsProcessSource, WindowsContextSource>;

/// Intervalo entre ciclos de coleta. Coleta de processos é a parte mais cara.
const TICK: Duration = Duration::from_secs(2);

struct Runtime {
    engine: Mutex<WinEngine>,
    start: Instant,
    stop: AtomicBool,
}

impl Runtime {
    fn mono_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

fn utc_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Partes da interface que precisam ser atualizadas quando o estado muda.
struct Ui {
    tray: TrayIcon,
    status: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    resume: MenuItem<Wry>,
    last_light: Mutex<Option<Light>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusDto {
    state: String,
    reason: String,
    text: String,
    light: &'static str,
    manually_paused: bool,
    can_pause: bool,
    can_resume: bool,
}

fn status_dto(state: RecorderState, reason: ReasonCode, manually_paused: bool) -> StatusDto {
    let model = menu_model(state, reason, manually_paused);
    StatusDto {
        state: format!("{state:?}"),
        reason: format!("{reason:?}"),
        text: bb_tray::describe(state, reason),
        light: match light_for(state) {
            Light::Green => "green",
            Light::Red => "red",
            Light::Gray => "gray",
        },
        manually_paused,
        can_pause: model.can_pause,
        can_resume: model.can_resume,
    }
}

/// Tamanho de ícone que o Windows usa de fato (16 a 100%, 20 a 125%, 24 a 150%, 32 a 200%).
fn system_icon_size(large: bool) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXICON, SM_CXSMICON};
    // SAFETY: consulta simples de métrica do sistema.
    let px = unsafe { GetSystemMetrics(if large { SM_CXICON } else { SM_CXSMICON }) };
    if px > 0 { px as u32 } else if large { 32 } else { 16 }
}

fn make_icon(light: Light, size: u32) -> Image<'static> {
    Image::new_owned(icon::render(light, size), size, size)
}

/// Lê o estado real do engine e atualiza bandeja, ícone da janela e a interface.
fn refresh(app: &AppHandle) -> StatusDto {
    let rt = app.state::<Arc<Runtime>>();
    let (state, reason, paused) = {
        let e = rt.engine.lock().expect("engine lock");
        let (s, r) = e.state(rt.mono_ms());
        (s, r, e.is_manually_paused())
    };
    let model = menu_model(state, reason, paused);
    let light = light_for(state);

    if let Some(ui) = app.try_state::<Ui>() {
        let _ = ui.status.set_text(&model.status);
        let _ = ui.pause.set_enabled(model.can_pause);
        let _ = ui.resume.set_enabled(model.can_resume);
        let _ = ui.tray.set_tooltip(Some(tooltip(state, reason)));
        let mut last = ui.last_light.lock().expect("light lock");
        if *last != Some(light) {
            let _ = ui.tray.set_icon(Some(make_icon(light, system_icon_size(false))));
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_icon(make_icon(light, system_icon_size(true)));
            }
            *last = Some(light);
        }
    }

    let dto = status_dto(state, reason, paused);
    let _ = app.emit("status", dto.clone());
    dto
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn do_pause(app: &AppHandle) -> StatusDto {
    app.state::<Arc<Runtime>>().engine.lock().expect("engine lock").pause();
    refresh(app)
}

/// A retomada só libera a pausa manual; o Privacy Guard decide se a gravação volta.
fn do_resume(app: &AppHandle) -> StatusDto {
    {
        let rt = app.state::<Arc<Runtime>>();
        let mono = rt.mono_ms();
        rt.engine.lock().expect("engine lock").resume(mono);
    }
    refresh(app)
}

fn quit(app: &AppHandle) {
    let rt = app.state::<Arc<Runtime>>();
    rt.stop.store(true, Ordering::SeqCst);
    // Sela o journal e finaliza capturas pendentes antes de sair.
    let _ = rt.engine.lock().expect("engine lock").shutdown();
    app.exit(0);
}

#[tauri::command]
fn get_status(app: AppHandle) -> StatusDto {
    refresh(&app)
}

#[tauri::command]
fn pause_recording(app: AppHandle) -> StatusDto {
    do_pause(&app)
}

#[tauri::command]
fn resume_recording(app: AppHandle) -> StatusDto {
    do_resume(&app)
}

fn data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("DeveloperBlackBox"))
}

fn build_engine() -> Result<WinEngine, String> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("data dir: {e}"))?;
    let keys = DpapiKeyStore::new(dir.join("key.bin"));
    let recorder = Recorder::open(dir.join("recorder"), &keys, RecorderConfig::default())
        .map_err(|e| format!("recorder: {e}"))?;
    // Os campos sensíveis do banco (nome de app, anotações, listas) usam a mesma chave DPAPI.
    let master = keys.key().map_err(|e| format!("key: {e}"))?;
    let store = Store::open_encrypted(dir.join("meta.db"), &master).map_err(|e| format!("store: {e}"))?;
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut engine = Engine::new(
        GuardConfig::default(),
        MetricsConfig::default(),
        recorder,
        WindowsProcessSource::new(),
        WindowsContextSource::new(),
        ncpu,
    );
    engine.enable_incidents(store, IncidentConfig::default());
    engine.set_crash_source(Box::new(WindowsCrashSource::new()));
    engine.load_settings().map_err(|e| format!("settings: {e}"))?;
    // Início conservador: nada é gravado até você autorizar em "Retomar gravação",
    // a menos que você tenha ligado o início automático nas configurações.
    if !engine.settings().auto_start {
        engine.pause();
    }
    Ok(engine)
}

fn build_tray(app: &tauri::App) -> tauri::Result<Ui> {
    let open = MenuItem::with_id(app, "open", "Abrir Black Box", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "Status: iniciando", false, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pausar gravação", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "Retomar gravação", true, None::<&str>)?;
    let privacy = MenuItem::with_id(app, "privacy", "Abrir configurações de privacidade", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sep1, &status, &pause, &resume, &sep2, &privacy, &quit_item])?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(make_icon(Light::Gray, system_icon_size(false)))
        .tooltip("Developer Black Box: iniciando")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "pause" => {
                do_pause(app);
            }
            "resume" => {
                do_resume(app);
            }
            "privacy" => {
                show_main(app);
                let _ = app.emit("navigate", "privacy");
            }
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(Ui { tray, status, pause, resume, last_light: Mutex::new(None) })
}

pub fn run() {
    let engine = match build_engine() {
        Ok(e) => e,
        Err(msg) => {
            // Sem chave/recorder seguro não há como gravar: não inicia (fail-closed).
            eprintln!("Developer Black Box could not start safely: {msg}");
            std::process::exit(1);
        }
    };
    let rt = Arc::new(Runtime { engine: Mutex::new(engine), start: Instant::now(), stop: AtomicBool::new(false) });

    tauri::Builder::default()
        // Duas instâncias disputariam o mesmo journal.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .manage(rt.clone())
        .invoke_handler(tauri::generate_handler![
            get_status,
            pause_recording,
            resume_recording,
            commands::get_overview,
            commands::get_activity,
            commands::get_processes,
            commands::list_incidents,
            commands::get_incident,
            commands::set_incident_state,
            commands::add_incident_note,
            commands::delete_incident,
            commands::capture_incident,
            commands::export_incident,
            commands::get_settings,
            commands::set_settings,
            commands::get_config_history,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
            commands::list_authorizations,
            commands::authorize_app,
            commands::revoke_authorization,
            commands::get_storage,
            commands::verify_integrity,
            commands::delete_activity,
        ])
        .on_window_event(|window, event| {
            // Fechar a janela só a esconde: o recorder e a bandeja continuam.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let ui = build_tray(app)?;
            app.manage(ui);
            refresh(app.handle());
            heal_startup_entry();
            // Aberto pelo Windows no login: fica só na bandeja. Aberto por você: mostra a janela.
            if !std::env::args().any(|a| a == MINIMIZED_ARG) {
                show_main(app.handle());
            }

            let handle = app.handle().clone();
            let worker = rt.clone();
            std::thread::spawn(move || {
                while !worker.stop.load(Ordering::SeqCst) {
                    {
                        let mut e = worker.engine.lock().expect("engine lock");
                        // Erros viram o estado de falha (ícone vermelho); não há conteúdo a registrar.
                        let _ = e.tick(worker.mono_ms(), utc_ms());
                    }
                    refresh(&handle);
                    std::thread::sleep(TICK);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Developer Black Box");
}
