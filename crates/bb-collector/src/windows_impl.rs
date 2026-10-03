//! Fontes reais do Windows.
//!
//! Privacidade: só o *nome do arquivo* do executável sai destas funções. Caminhos
//! completos, linhas de comando e títulos de janela nunca são lidos para fora.

use std::mem::size_of;

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    GetProcessTimes, GetSystemTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};

use bb_core::{ExeName, Observation, ProcessKey};

use crate::hosted::{resolve_hosted, HostedChild, CORE_WINDOW_CLASS, FRAME_HOST_EXE};
use crate::sample::{CollectError, ContextSource, ProcessSample, ProcessSource, SystemSample};

/// FILETIME (100 ns desde 1601) -> u64.
fn ft(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

/// FILETIME -> milissegundos desde a época Unix (UTC).
fn ft_to_unix_ms(f: FILETIME) -> i64 {
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    (ft(f).saturating_sub(EPOCH_DIFF_100NS) / 10_000) as i64
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: handle obtido por OpenProcess e fechado uma única vez.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open_limited(pid: u32) -> Option<Handle> {
    // SAFETY: chamada simples; falha (acesso negado etc.) vira None.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok().map(Handle) }
}

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

pub struct WindowsProcessSource {
    prev_sys: Option<(u64, u64, u64)>, // idle, kernel (inclui idle), user
}

impl WindowsProcessSource {
    pub fn new() -> Self {
        Self { prev_sys: None }
    }
}

impl Default for WindowsProcessSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSource for WindowsProcessSource {
    /// Processos que não podem ser abertos (protegidos/elevados) são omitidos por
    /// inteiro: sem horário de início não há identidade confiável, e não-gravar é
    /// a opção conservadora.
    fn processes(&mut self) -> Result<Vec<ProcessSample>, CollectError> {
        // SAFETY: APIs Win32 com buffers próprios e handles fechados por RAII.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                .map_err(|e| CollectError(format!("snapshot: {e}")))?;
            let _guard = Handle(snap);
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut out = Vec::new();
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                if let Some(sample) = sample_of(&entry) {
                    out.push(sample);
                }
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            Ok(out)
        }
    }

    fn system(&mut self) -> Result<SystemSample, CollectError> {
        // SAFETY: structs inicializadas localmente.
        unsafe {
            let mut idle = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user))
                .map_err(|e| CollectError(format!("GetSystemTimes: {e}")))?;
            let now = (ft(idle), ft(kernel), ft(user));
            let cpu_permille = match self.prev_sys {
                Some((pi, pk, pu)) => {
                    let total = now.1.saturating_sub(pk) + now.2.saturating_sub(pu);
                    let idle_d = now.0.saturating_sub(pi);
                    if total == 0 { 0 } else { ((total.saturating_sub(idle_d)) * 1000 / total).min(1000) as u16 }
                }
                None => 0,
            };
            self.prev_sys = Some(now);

            let mut mem = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            GlobalMemoryStatusEx(&mut mem).map_err(|e| CollectError(format!("GlobalMemoryStatusEx: {e}")))?;
            Ok(SystemSample {
                cpu_permille,
                mem_used_kb: mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys) / 1024,
                mem_total_kb: mem.ullTotalPhys / 1024,
            })
        }
    }
}

/// # Safety
/// `entry` deve vir de Process32FirstW/NextW.
unsafe fn sample_of(entry: &PROCESSENTRY32W) -> Option<ProcessSample> {
    let pid = entry.th32ProcessID;
    let exe_name = ExeName::new(&wide_to_string(&entry.szExeFile)).ok()?;
    let h = open_limited(pid)?;
    let (mut created, mut exit, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    GetProcessTimes(h.0, &mut created, &mut exit, &mut kernel, &mut user).ok()?;
    let mut mem = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    let working_set_kb = match GetProcessMemoryInfo(h.0, &mut mem, mem.cb) {
        Ok(()) => mem.WorkingSetSize as u64 / 1024,
        Err(_) => 0,
    };
    Some(ProcessSample {
        key: ProcessKey { pid, start_time_ms: ft_to_unix_ms(created) },
        exe_name,
        parent_pid: entry.th32ParentProcessID,
        cpu_time_100ns: ft(kernel) + ft(user),
        working_set_kb,
    })
}

/// Sinais de contexto para o Privacy Guard: nada além do nome do executável em
/// primeiro plano (e só para decisão em memória) e se a sessão está bloqueada.
pub struct WindowsContextSource;

impl WindowsContextSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsContextSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextSource for WindowsContextSource {
    fn observe(&mut self) -> Observation {
        Observation { detector_ok: true, session_locked: input_desktop_unavailable(), foreground: foreground_exe() }
    }
}

/// Com a sessão bloqueada, ou numa tela segura (UAC), não é possível abrir o
/// desktop de entrada. Qualquer falha conta como bloqueado (fail-closed).
fn input_desktop_unavailable() -> bool {
    // SAFETY: o handle é fechado imediatamente.
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
            Ok(d) => {
                let _ = CloseDesktop(d);
                false
            }
            Err(_) => true,
        }
    }
}

/// Nome do executável da janela em primeiro plano AGORA, pela mesma função que o Guard usa. Serve ao botão "Detectar o
/// app em primeiro plano": o que ela devolve é exatamente o que as regras de privacidade comparam.
pub fn current_foreground_exe() -> Option<ExeName> {
    foreground_exe()
}

/// Nome do executável da janela em primeiro plano. Nunca lê o título da janela e
/// descarta o caminho. `None` se não puder ser identificado (o Guard trata como
/// desconhecido e bloqueia).
fn foreground_exe() -> Option<ExeName> {
    // SAFETY: buffers locais; handle fechado por RAII.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let name = exe_name_of(pid)?;
        if name.as_str() != FRAME_HOST_EXE {
            return Some(name);
        }
        // App UWP: a janela em primeiro plano é do hospedeiro; o app é uma janela filha de OUTRO processo. Sem um app
        // identificável, devolve None (desconhecido: o Guard bloqueia) em vez de tratar o hospedeiro como se fosse o app.
        let app_pid = resolve_hosted(pid, &hosted_children(hwnd))?;
        exe_name_of(app_pid)
    }
}

/// Nome do executável (sem caminho) do processo `pid`.
fn exe_name_of(pid: u32) -> Option<ExeName> {
    // SAFETY: buffers locais; handle fechado por RAII.
    unsafe {
        let h = open_limited(pid)?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()?;
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        ExeName::new(full.rsplit(['\\', '/']).next()?).ok()
    }
}

/// Janelas filhas da moldura: só o processo dono e se a classe é a do app UWP. Nunca o título.
fn hosted_children(frame: HWND) -> Vec<HostedChild> {
    unsafe extern "system" fn each(child: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` é o ponteiro para o Vec que `hosted_children` mantém vivo durante toda a enumeração.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<HostedChild>) };
        let mut pid = 0u32;
        // SAFETY: `child` é um handle válido durante o callback; buffers locais.
        unsafe {
            GetWindowThreadProcessId(child, Some(&mut pid));
            let mut class = [0u16; 64];
            let n = GetClassNameW(child, &mut class);
            let is_core = n > 0 && String::from_utf16_lossy(&class[..n as usize]) == CORE_WINDOW_CLASS;
            out.push(HostedChild { pid, is_core_window: is_core });
        }
        BOOL(1)
    }
    let mut out: Vec<HostedChild> = Vec::new();
    // SAFETY: o ponteiro para `out` só é usado dentro desta chamada síncrona.
    unsafe {
        let _ = EnumChildWindows(Some(frame), Some(each), LPARAM(&mut out as *mut Vec<HostedChild> as isize));
    }
    out
}
