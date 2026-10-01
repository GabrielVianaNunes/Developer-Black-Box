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
use bb_tray::{icon, light_for, menu_labels, menu_model, tooltip, Lang, Light};

mod apps;
mod commands;
mod guide;
mod updates;

/// Chave da configuração salva com o idioma escolhido (guardada cifrada, como as demais).
pub const LANGUAGE_SETTING: &str = "language";

/// Idioma do Windows do usuário: português (qualquer variante) usa pt-BR; o resto usa inglês.
/// Só vale na primeira execução, até o usuário escolher um idioma no app.
fn detect_system_language() -> Lang {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    // SAFETY: buffer local com o tamanho máximo de nome de localidade (LOCALE_NAME_MAX_LENGTH).
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if n <= 1 {
        return Lang::En;
    }
    Lang::from_locale(&String::from_utf16_lossy(&buf[..(n - 1) as usize]))
}

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
    /// Idioma atual da interface (o painel e os textos da bandeja).
    lang: Mutex<Lang>,
}

impl Runtime {
    fn mono_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn lang(&self) -> Lang {
        *self.lang.lock().expect("lang lock")
    }
}

fn utc_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Partes da interface que precisam ser atualizadas quando o estado muda.
struct Ui {
    tray: TrayIcon,
    open: MenuItem<Wry>,
    status: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    resume: MenuItem<Wry>,
    privacy: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    last_light: Mutex<Option<Light>>,
    /// Idioma com que os textos fixos do menu foram escritos pela última vez.
    last_lang: Mutex<Option<Lang>>,
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

fn status_dto(lang: Lang, state: RecorderState, reason: ReasonCode, manually_paused: bool) -> StatusDto {
    let model = menu_model(lang, state, reason, manually_paused);
    StatusDto {
        state: format!("{state:?}"),
        reason: format!("{reason:?}"),
        text: bb_tray::describe(lang, state, reason),
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
    let lang = rt.lang();
    let model = menu_model(lang, state, reason, paused);
    let light = light_for(state);

    if let Some(ui) = app.try_state::<Ui>() {
        // Textos fixos do menu: reescritos só quando o idioma muda.
        let mut last_lang = ui.last_lang.lock().expect("lang lock");
        if *last_lang != Some(lang) {
            let labels = menu_labels(lang);
            let _ = ui.open.set_text(labels.open);
            let _ = ui.pause.set_text(labels.pause);
            let _ = ui.resume.set_text(labels.resume);
            let _ = ui.privacy.set_text(labels.privacy);
            let _ = ui.quit.set_text(labels.quit);
            *last_lang = Some(lang);
        }
        drop(last_lang);
        let _ = ui.status.set_text(&model.status);
        let _ = ui.pause.set_enabled(model.can_pause);
        let _ = ui.resume.set_enabled(model.can_resume);
        let _ = ui.tray.set_tooltip(Some(tooltip(lang, state, reason)));
        #[cfg(feature = "e2e")]
        dump_menu_texts(&ui);
        let mut last = ui.last_light.lock().expect("light lock");
        if *last != Some(light) {
            let _ = ui.tray.set_icon(Some(make_icon(light, system_icon_size(false))));
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_icon(make_icon(light, system_icon_size(true)));
            }
            *last = Some(light);
        }
    }

    let dto = status_dto(lang, state, reason, paused);
    let _ = app.emit("status", dto.clone());
    dto
}

/// Verificação manual (feature `e2e`): relê o texto dos itens nativos do menu e grava em disco.
#[cfg(feature = "e2e")]
fn dump_menu_texts(ui: &Ui) {
    let t = |i: &MenuItem<Wry>| i.text().unwrap_or_default();
    let line = [t(&ui.open), t(&ui.status), t(&ui.pause), t(&ui.resume), t(&ui.privacy), t(&ui.quit)].join("|");
    if let Ok(dir) = data_dir() {
        let _ = std::fs::write(dir.join("e2e-tray.txt"), line);
    }
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

pub(crate) fn quit(app: &AppHandle) {
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

pub(crate) fn data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("DeveloperBlackBox"))
}

fn build_engine() -> Result<(WinEngine, Lang), String> {
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
    // Idioma: o que você escolheu (salvo); na primeira execução, o do Windows.
    let lang = engine
        .store()
        .and_then(|s| s.get_setting(LANGUAGE_SETTING).ok().flatten())
        .and_then(|code| Lang::parse(&code))
        .unwrap_or_else(detect_system_language);
    Ok((engine, lang))
}

fn build_tray(app: &tauri::App, lang: Lang) -> tauri::Result<Ui> {
    let l = menu_labels(lang);
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", l.starting_status, false, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", l.pause, true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", l.resume, true, None::<&str>)?;
    let privacy = MenuItem::with_id(app, "privacy", l.privacy, true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sep1, &status, &pause, &resume, &sep2, &privacy, &quit_item])?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(make_icon(Light::Gray, system_icon_size(false)))
        .tooltip(l.starting_tooltip)
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

    Ok(Ui {
        tray,
        open,
        status,
        pause,
        resume,
        privacy,
        quit: quit_item,
        last_light: Mutex::new(None),
        last_lang: Mutex::new(Some(lang)),
    })
}

pub fn run() {
    let (engine, lang) = match build_engine() {
        Ok(e) => e,
        Err(msg) => {
            // Sem chave/recorder seguro não há como gravar: não inicia (fail-closed).
            eprintln!("Developer Black Box could not start safely: {msg}");
            std::process::exit(1);
        }
    };
    let rt = Arc::new(Runtime {
        engine: Mutex::new(engine),
        start: Instant::now(),
        stop: AtomicBool::new(false),
        lang: Mutex::new(lang),
    });

    tauri::Builder::default()
        // Duas instâncias disputariam o mesmo journal.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .manage(rt.clone())
        .manage(Arc::new(updates::Updates::default()))
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
            commands::get_app_version,
            guide::get_guide_state,
            guide::mark_tour_seen,
            apps::list_app_candidates,
            apps::pick_executable,
            updates::get_update_state,
            updates::set_update_check,
            updates::check_for_updates,
            updates::open_release_page,
            updates::download_update,
            updates::install_update,
            commands::get_language,
            commands::set_language,
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
            let ui = build_tray(app, rt.lang())?;
            app.manage(ui);
            refresh(app.handle());
            heal_startup_entry();
            // Aberto pelo Windows no login: fica só na bandeja. Aberto por você: mostra a janela.
            if !std::env::args().any(|a| a == MINIMIZED_ARG) {
                show_main(app.handle());
            }

            updates::spawn_background(app.handle().clone(), rt.clone());

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
