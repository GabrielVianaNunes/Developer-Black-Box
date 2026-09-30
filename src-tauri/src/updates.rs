//! Verificação de atualizações: só AVISA que existe uma versão nova. Não baixa nem instala nada.
//!
//! Privacidade: a verificação automática vem DESLIGADA e só roda se você ligar. O botão "Verificar agora"
//! é uma ação sua e funciona mesmo com a automática desligada. Sem nenhuma das duas, o app não abre
//! nenhuma conexão de rede. O que é enviado está descrito em `bb_update`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bb_update::{check, current_version, prepare, release_url, reverify, Outcome, Version, WinHttpFetcher, INSTALLER_ARGS};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{data_dir, quit, utc_ms, Runtime};

pub const UPDATE_CHECK_SETTING: &str = "update_check";

/// Espera antes da primeira verificação automática, para não disputar o início do app.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
/// Entre verificações automáticas.
const INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;
/// Com que frequência a thread confere se chegou a hora (e se a opção foi ligada ou desligada).
const POLL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct State {
    latest: Option<Version>,
    checked_utc_ms: Option<i64>,
    error: Option<&'static str>,
    checking: bool,
    downloading: bool,
    /// Versão já baixada e verificada, pronta para instalar.
    ready: Option<Version>,
}

#[derive(Default)]
pub struct Updates {
    state: Mutex<State>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDto {
    /// Verificação automática ligada.
    enabled: bool,
    current: String,
    /// Há versão mais nova que a instalada.
    available: bool,
    latest: Option<String>,
    checked_utc_ms: Option<i64>,
    /// Código de erro neutro de idioma (ex. `update.network`), se a última verificação falhou.
    error: Option<String>,
    checking: bool,
    downloading: bool,
    /// O instalador da versão nova já foi baixado e verificado (SHA-256 + assinatura).
    ready: bool,
}

fn enabled(app: &AppHandle) -> bool {
    let rt = app.state::<Arc<Runtime>>();
    let e = rt.engine.lock().expect("engine lock");
    e.store().and_then(|s| s.get_setting(UPDATE_CHECK_SETTING).ok().flatten()).is_some_and(|v| v == "true")
}

fn dto(app: &AppHandle) -> UpdateDto {
    let enabled = enabled(app);
    let current = current_version();
    let updates = app.state::<Arc<Updates>>();
    let s = updates.state.lock().expect("update lock");
    UpdateDto {
        enabled,
        current: current.to_string(),
        available: s.latest.is_some_and(|l| l > current),
        latest: s.latest.map(|v| v.to_string()),
        checked_utc_ms: s.checked_utc_ms,
        error: s.error.map(str::to_owned),
        checking: s.checking,
        downloading: s.downloading,
        ready: s.ready.is_some_and(|r| s.latest == Some(r)),
    }
}

fn publish(app: &AppHandle) -> UpdateDto {
    let d = dto(app);
    let _ = app.emit("update", d.clone());
    d
}

/// Faz uma verificação agora (bloqueia até terminar; chame fora da thread da interface).
fn run_check(app: &AppHandle) -> UpdateDto {
    let updates = app.state::<Arc<Updates>>();
    {
        let mut s = updates.state.lock().expect("update lock");
        if s.checking {
            drop(s);
            return dto(app);
        }
        s.checking = true;
    }
    publish(app);

    // A rede roda sem segurar nenhum lock do app.
    let current = current_version();
    let result = check(&current, &fetcher(&current));

    {
        let mut s = updates.state.lock().expect("update lock");
        s.checking = false;
        s.checked_utc_ms = Some(utc_ms());
        match result {
            Ok(Outcome::Available { version }) => {
                s.latest = Some(version);
                s.error = None;
            }
            Ok(Outcome::UpToDate) => {
                s.latest = None;
                s.error = None;
            }
            // Falha: mantém o que já se sabia e só registra o motivo.
            Err(e) => s.error = Some(e.code()),
        }
    }
    publish(app)
}

/// Verificação automática em segundo plano. Só chama a rede se a opção estiver ligada.
pub fn spawn_background(app: AppHandle, rt: Arc<Runtime>) {
    std::thread::spawn(move || {
        // Ao abrir não há download em andamento: apaga o que sobrou (por exemplo, o instalador da atualização que acabou de rodar).
        if let Ok(dir) = updates_dir() {
            let _ = std::fs::remove_dir_all(dir);
        }
        let sleep_checked = |total: Duration| {
            let mut left = total;
            while left > Duration::ZERO && !rt.stop.load(std::sync::atomic::Ordering::SeqCst) {
                let step = left.min(Duration::from_secs(1));
                std::thread::sleep(step);
                left -= step;
            }
        };
        sleep_checked(FIRST_CHECK_DELAY);
        while !rt.stop.load(std::sync::atomic::Ordering::SeqCst) {
            if enabled(&app) {
                let due = {
                    let updates = app.state::<Arc<Updates>>();
                    let s = updates.state.lock().expect("update lock");
                    s.checked_utc_ms.is_none_or(|t| utc_ms() - t >= INTERVAL_MS)
                };
                if due {
                    run_check(&app);
                }
            }
            sleep_checked(POLL);
        }
    });
}

/// Transporte de rede. Em build de TESTE (feature `e2e`), `BB_E2E_UPDATE_BASE=host:porta` aponta para um servidor local.
fn fetcher(current: &Version) -> WinHttpFetcher {
    #[allow(unused_mut)]
    let mut f = WinHttpFetcher::new(&current.to_string());
    #[cfg(feature = "e2e")]
    if let Some((host, port)) = std::env::var("BB_E2E_UPDATE_BASE").ok().as_deref().and_then(|b| b.rsplit_once(':')) {
        if let Ok(port) = port.parse() {
            f = f.with_local_server(host, port);
        }
    }
    f
}

/// Pasta só das atualizações baixadas (dentro dos dados do app).
fn updates_dir() -> Result<std::path::PathBuf, String> {
    Ok(data_dir()?.join("updates"))
}

// ---- comandos ----

#[tauri::command]
pub fn get_update_state(app: AppHandle) -> UpdateDto {
    dto(&app)
}

/// Liga ou desliga a verificação automática (salva cifrada; o histórico guarda só "update_check: changed").
#[tauri::command]
pub fn set_update_check(app: AppHandle, enabled: bool) -> Result<UpdateDto, String> {
    {
        let rt = app.state::<Arc<Runtime>>();
        let e = rt.engine.lock().map_err(|_| "internal".to_string())?;
        if let Some(store) = e.store() {
            let was = store.get_setting(UPDATE_CHECK_SETTING).ok().flatten().is_some_and(|v| v == "true");
            store
                .set_setting(UPDATE_CHECK_SETTING, if enabled { "true" } else { "false" })
                .map_err(|_| "store.error".to_string())?;
            if was != enabled {
                let _ = store.log_config_change(utc_ms(), "update_check", "changed");
            }
        }
    }
    // Ligar não espera a hora marcada: a thread verifica no próximo ciclo se ainda não houve verificação.
    Ok(publish(&app))
}

/// "Verificar agora": ação do usuário, vale mesmo com a verificação automática desligada.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateDto, String> {
    tauri::async_runtime::spawn_blocking(move || run_check(&app)).await.map_err(|_| "internal".to_string())
}

/// Baixa o instalador da versão nova, verifica SHA-256 e assinatura e só então o deixa pronto. Ação do usuário.
/// Nada é executado aqui; qualquer falha apaga o que foi baixado.
#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<UpdateDto, String> {
    tauri::async_runtime::spawn_blocking(move || run_download(&app)).await.map_err(|_| "internal".to_string())?
}

fn run_download(app: &AppHandle) -> Result<UpdateDto, String> {
    let updates = app.state::<Arc<Updates>>();
    let latest = {
        let mut s = updates.state.lock().map_err(|_| "internal".to_string())?;
        let latest = s.latest.filter(|l| *l > current_version()).ok_or("update.none_available")?;
        if s.downloading || s.checking {
            drop(s);
            return Ok(dto(app));
        }
        s.downloading = true;
        s.ready = None;
        s.error = None;
        latest
    };
    publish(app);

    let dir = updates_dir();
    let current = current_version();
    let result = match dir {
        Ok(dir) => prepare(&latest, &dir, &fetcher(&current)).map(|_| ()),
        Err(_) => Err(bb_update::UpdateError::Disk),
    };
    {
        let mut s = updates.state.lock().map_err(|_| "internal".to_string())?;
        s.downloading = false;
        match result {
            Ok(()) => s.ready = Some(latest),
            Err(e) => s.error = Some(e.code()),
        }
    }
    Ok(publish(app))
}

/// Confere o instalador de novo, abre-o em modo atualização (só progresso, sem perguntas, reabre o app
/// no fim) e fecha este app para o instalador poder substituir os arquivos. Ação do usuário.
#[tauri::command]
pub fn install_update(app: AppHandle) -> Result<(), String> {
    let ready = {
        let updates = app.state::<Arc<Updates>>();
        let s = updates.state.lock().map_err(|_| "internal".to_string())?;
        s.ready.filter(|r| s.latest == Some(*r) && *r > current_version())
    };
    let version = ready.ok_or("update.not_ready")?;
    let installer = reverify(&version, &updates_dir()?).map_err(|e| e.code().to_string())?;
    std::process::Command::new(&installer).args(INSTALLER_ARGS).spawn().map_err(|_| "update.launch_failed".to_string())?;
    // Dá um instante ao instalador para abrir e encerra o app de forma limpa (sela o journal).
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        quit(&app);
    });
    Ok(())
}

/// Abre no navegador a página da Release da versão nova. O endereço é montado a partir da versão
/// validada (nunca de texto vindo da rede) e é sempre o do repositório do projeto.
#[tauri::command]
pub fn open_release_page(app: AppHandle) -> Result<(), String> {
    let latest = {
        let updates = app.state::<Arc<Updates>>();
        let s = updates.state.lock().map_err(|_| "internal".to_string())?;
        s.latest
    };
    let latest = latest.filter(|l| *l > current_version()).ok_or("update.none_available")?;
    open_in_browser(&release_url(&latest)).map_err(|_| "update.open_failed".to_string())
}

fn open_in_browser(url: &str) -> Result<(), ()> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    debug_assert!(url.starts_with("https://github.com/"));
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: cadeia UTF-16 terminada em zero que vive até o fim da chamada.
    let result = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL) };
    // ShellExecute devolve um valor > 32 em caso de sucesso.
    if result.0 as usize > 32 { Ok(()) } else { Err(()) }
}

