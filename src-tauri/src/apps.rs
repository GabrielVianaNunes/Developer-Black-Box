//! Comandos para ESCOLHER programas nas listas de privacidade, em vez de digitar nomes.
//!
//! A lista de programas é montada na hora, só na memória, e nunca é gravada nem enviada. O seletor de arquivos
//! é o do Windows; só o NOME do `.exe` escolhido volta para a interface (nunca a pasta).

use bb_collector::apps::{list_candidates, picked_exe};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CandidateDto {
    exe: String,
    name: String,
    running: bool,
    installed: bool,
}

/// Programas instalados e em execução neste PC (ver `bb_collector::apps`).
#[tauri::command]
pub async fn list_app_candidates() -> Result<Vec<CandidateDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        list_candidates()
            .into_iter()
            .map(|c| CandidateDto { exe: c.exe, name: c.name, running: c.running, installed: c.installed })
            .collect()
    })
    .await
    .map_err(|_| "apps.list_failed".to_string())
}

/// Abre o seletor de arquivos do Windows (filtro `*.exe`). Devolve o nome do executável em minúsculas,
/// `None` se o usuário cancelar, ou o erro `apps.invalid_name` se o arquivo não for um nome aceito pelo Guard.
#[tauri::command]
pub async fn pick_executable(app: AppHandle, title: String, filter_label: String) -> Result<Option<String>, String> {
    let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize);
    // O seletor de arquivos do Windows precisa de um thread com COM (apartamento STA); os threads do pool
    // genérico não têm. Um thread próprio, com COM inicializado, garante que o diálogo apareça.
    let picked = tauri::async_runtime::spawn_blocking(move || {
        std::thread::Builder::new()
            .name("file-picker".into())
            .spawn(move || {
                use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
                // SAFETY: inicializa e desfaz o COM neste mesmo thread, em volta do diálogo.
                let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
                let result = open_dialog(hwnd, &title, &filter_label);
                if com_ok {
                    unsafe { CoUninitialize() };
                }
                result
            })
            .map_err(|_| "apps.pick_failed".to_string())?
            .join()
            .map_err(|_| "apps.pick_failed".to_string())?
    })
    .await
    .map_err(|_| "apps.pick_failed".to_string())??;
    match picked {
        None => Ok(None),
        Some(path) => picked_exe(&path).map(Some).ok_or_else(|| "apps.invalid_name".to_string()),
    }
}

fn open_dialog(owner: Option<isize>, title: &str, filter_label: &str) -> Result<Option<String>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    // Só texto simples nos rótulos (eles vêm da interface): sem caracteres de controle e com tamanho limitado.
    let clean = |s: &str| s.chars().filter(|c| !c.is_control()).take(80).collect::<String>();
    let title_w: Vec<u16> = clean(title).encode_utf16().chain(std::iter::once(0)).collect();
    // Filtro: "rótulo\0*.exe\0\0".
    let mut filter_w: Vec<u16> = clean(filter_label).encode_utf16().collect();
    filter_w.push(0);
    filter_w.extend("*.exe".encode_utf16());
    filter_w.extend([0, 0]);

    let mut file = [0u16; 1024];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(owner.unwrap_or(0) as *mut _),
        lpstrFilter: PCWSTR(filter_w.as_ptr()),
        lpstrFile: windows::core::PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrTitle: PCWSTR(title_w.as_ptr()),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_HIDEREADONLY,
        ..Default::default()
    };
    // SAFETY: `ofn` aponta para buffers que vivem durante a chamada; o diálogo é modal e devolve antes de sairmos.
    let chosen = unsafe { GetOpenFileNameW(&mut ofn) }.as_bool();
    if !chosen {
        return Ok(None); // cancelado (ou fechado)
    }
    let end = file.iter().position(|&u| u == 0).unwrap_or(file.len());
    Ok(Some(String::from_utf16_lossy(&file[..end])))
}
