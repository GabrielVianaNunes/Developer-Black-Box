//! Programas deste PC, para o usuário ESCOLHER em vez de digitar nomes (listas de privacidade).
//!
//! Fontes (todas locais): processos em execução, `App Paths` e a lista de programas instalados do registro.
//! O resultado só existe em memória: nada é gravado em disco, registrado em log ou enviado. Só entram nomes
//! que o Guard aceitaria (`ExeName`), sempre em minúsculas e terminados em `.exe`.

use std::collections::HashMap;

use bb_core::ExeName;

/// Limite de candidatos devolvidos (um PC com milhares de entradas não trava a interface).
pub const MAX_CANDIDATES: usize = 5000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppCandidate {
    /// Nome do executável em minúsculas (ex.: `chrome.exe`). É o que as listas guardam.
    pub exe: String,
    /// Nome para mostrar ao usuário (ex.: `Google Chrome`); o nome do arquivo sem `.exe` se não houver outro.
    pub name: String,
    /// Está em execução agora.
    pub running: bool,
    /// Está instalado (aparece no registro de programas instalados ou em `App Paths`).
    pub installed: bool,
}

/// Nome do arquivo `.exe` de um caminho do registro como `"C:\Program Files\App\app.exe",0`, `C:\x\app.exe,-101`
/// ou `C:\x\app.exe`. Variáveis de ambiente (`%SystemRoot%`) não são expandidas: só o nome do arquivo importa.
pub fn exe_from_icon_path(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix('"') {
        s = rest.split('"').next()?;
    } else if let Some((path, index)) = s.rsplit_once(',') {
        // `caminho,0` ou `caminho,-101`: o que vem depois da vírgula é o índice do ícone.
        if index.trim().trim_start_matches('-').bytes().all(|b| b.is_ascii_digit()) && !index.trim().is_empty() {
            s = path;
        }
    }
    let file = s.trim().rsplit(['\\', '/']).next()?.trim();
    let lower = file.to_lowercase();
    (lower.len() > ".exe".len() && lower.ends_with(".exe")).then_some(lower)
}

/// Nome de executável a partir de um arquivo escolhido no seletor do Windows. Só o NOME do arquivo é guardado
/// (nunca a pasta) e ele passa pela mesma validação do Guard. `None` se não for um `.exe` válido.
pub fn picked_exe(path: &str) -> Option<String> {
    let exe = exe_from_icon_path(path)?;
    ExeName::new(&exe).ok().map(|n| n.as_str().to_owned())
}

/// Nome amigável de reserva: o arquivo sem `.exe`.
pub fn stem(exe: &str) -> String {
    exe.strip_suffix(".exe").unwrap_or(exe).to_owned()
}

/// Junta as fontes sem repetir (chave: exe em minúsculas), descarta nomes que o Guard recusaria e ordena:
/// programas instalados primeiro, depois os que só estão em execução; dentro de cada grupo, por nome.
pub fn merge(running: &[String], installed: &[(String, String)]) -> Vec<AppCandidate> {
    let mut by_exe: HashMap<String, AppCandidate> = HashMap::new();
    let mut add = |exe: &str, name: Option<&str>, running: bool, installed: bool| {
        let exe = exe.trim().to_lowercase();
        if !exe.ends_with(".exe") || ExeName::new(&exe).is_err() {
            return;
        }
        let entry = by_exe.entry(exe.clone()).or_insert_with(|| AppCandidate {
            name: stem(&exe),
            exe,
            running: false,
            installed: false,
        });
        entry.running |= running;
        entry.installed |= installed;
        // O primeiro nome amigável real vence (não troca por outro nem volta para o nome do arquivo).
        if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
            if entry.name == stem(&entry.exe) {
                entry.name = n.to_owned();
            }
        }
    };
    for (exe, name) in installed {
        add(exe, Some(name), false, true);
    }
    for exe in running {
        add(exe, None, true, false);
    }
    let mut out: Vec<AppCandidate> = by_exe.into_values().collect();
    out.sort_by(|a, b| (!a.installed, a.name.to_lowercase(), &a.exe).cmp(&(!b.installed, b.name.to_lowercase(), &b.exe)));
    out.truncate(MAX_CANDIDATES);
    out
}

#[cfg(windows)]
pub use win::list_candidates;

#[cfg(windows)]
mod win {
    use std::mem::size_of;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };

    use super::{exe_from_icon_path, merge, AppCandidate};

    const APP_PATHS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths";
    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    const APP_PATHS_32: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths";
    const UNINSTALL_32: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: handle aberto por RegOpenKeyExW e fechado uma única vez.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(root: HKEY, path: &str) -> Option<Key> {
        let p = wide(path);
        let mut h = HKEY::default();
        // SAFETY: `p` é terminado em NUL e vive durante a chamada.
        let status = unsafe { RegOpenKeyExW(root, PCWSTR(p.as_ptr()), None, KEY_READ, &mut h) };
        (status == ERROR_SUCCESS).then_some(Key(h))
    }

    fn subkeys(key: &Key) -> Vec<String> {
        let mut out = Vec::new();
        for index in 0.. {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            // SAFETY: `buf` tem `len` unidades; o tamanho devolvido não inclui o NUL final.
            let status = unsafe { RegEnumKeyExW(key.0, index, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
            if status != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    fn read_string(key: &Key, name: &str) -> Option<String> {
        let n = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut len = 0u32;
        // SAFETY: primeira chamada só pergunta o tamanho.
        let probe = unsafe { RegQueryValueExW(key.0, PCWSTR(n.as_ptr()), None, Some(&mut kind), None, Some(&mut len)) };
        if probe != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) || len == 0 || len > 32 * 1024 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` tem `len` bytes.
        let status = unsafe {
            RegQueryValueExW(key.0, PCWSTR(n.as_ptr()), None, Some(&mut kind), Some(buf.as_mut_ptr()), Some(&mut len))
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let units: Vec<u16> = buf[..len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        Some(String::from_utf16_lossy(&units[..end]))
    }

    /// Nomes de executáveis em execução. Lê só o nome da foto da lista de processos: não abre nenhum processo,
    /// então também inclui os elevados.
    fn running_exes() -> Vec<String> {
        let mut out = Vec::new();
        // SAFETY: APIs Win32 com buffer próprio; o handle da foto é fechado ao final.
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                let end = entry.szExeFile.iter().position(|&u| u == 0).unwrap_or(entry.szExeFile.len());
                out.push(String::from_utf16_lossy(&entry.szExeFile[..end]));
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = windows::Win32::Foundation::CloseHandle(snap);
        }
        out
    }

    /// Programas instalados: (exe, nome amigável). `App Paths` só dá o exe; a lista de desinstalação dá nome e,
    /// quando o ícone aponta para o executável, o exe.
    fn installed() -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (root, path) in [
            (HKEY_LOCAL_MACHINE, APP_PATHS),
            (HKEY_LOCAL_MACHINE, APP_PATHS_32),
            (HKEY_CURRENT_USER, APP_PATHS),
        ] {
            let Some(key) = open(root, path) else { continue };
            for name in subkeys(&key) {
                if exe_from_icon_path(&name).is_some() {
                    out.push((name, String::new()));
                }
            }
        }
        for (root, path) in [
            (HKEY_LOCAL_MACHINE, UNINSTALL),
            (HKEY_LOCAL_MACHINE, UNINSTALL_32),
            (HKEY_CURRENT_USER, UNINSTALL),
        ] {
            let Some(parent) = open(root, path) else { continue };
            for sub in subkeys(&parent) {
                let Some(entry) = open(root, &format!("{path}\\{sub}")) else { continue };
                let (Some(display), Some(icon)) = (read_string(&entry, "DisplayName"), read_string(&entry, "DisplayIcon")) else {
                    continue;
                };
                if let Some(exe) = exe_from_icon_path(&icon) {
                    out.push((exe, display));
                }
            }
        }
        out
    }

    /// Candidatos para as listas de privacidade. Rápido (milissegundos) e sem efeitos colaterais.
    pub fn list_candidates() -> Vec<AppCandidate> {
        merge(&running_exes(), &installed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_owned()
    }

    #[test]
    fn exe_is_extracted_from_the_shapes_the_registry_uses() {
        let cases = [
            ("\"C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe\",0", Some("chrome.exe")),
            ("C:\\Program Files\\App\\App.EXE,0", Some("app.exe")),
            ("C:\\x\\tool.exe,-101", Some("tool.exe")),
            ("C:\\x\\tool.exe", Some("tool.exe")),
            ("  \"C:\\x\\spaced name.exe\"  ", Some("spaced name.exe")),
            ("%SystemRoot%\\system32\\notepad.exe,0", Some("notepad.exe")),
            ("\"C:\\x\\a.exe\",1", Some("a.exe")),
            ("C:/forward/slashes/b.exe", Some("b.exe")),
            ("C:\\x\\icon.ico", None),
            ("C:\\x\\installer.msi,0", None),
            ("C:\\x\\dir\\", None),
            ("", None),
            (".exe", None),
            ("C:\\x\\noext,0", None),
        ];
        for (raw, want) in cases {
            assert_eq!(exe_from_icon_path(raw).as_deref(), want, "{raw:?}");
        }
    }

    #[test]
    fn a_picked_file_keeps_only_its_validated_name() {
        assert_eq!(picked_exe("C:\\Program Files\\My App\\MyApp.EXE").as_deref(), Some("myapp.exe"));
        assert_eq!(picked_exe("D:\\portable\\tool,v2.exe").as_deref(), Some("tool,v2.exe"));
        for bad in ["", "C:\\x\\doc.pdf", "C:\\x\\", "not a path", "C:\\x\\setup.msi", ".exe"] {
            assert_eq!(picked_exe(bad), None, "{bad:?}");
        }
        let long = format!("C:\\x\\{}.exe", "a".repeat(80));
        assert_eq!(picked_exe(&long), None, "names the Guard refuses are refused here too");
    }

    #[test]
    fn merge_dedupes_keeps_the_friendly_name_and_marks_running() {
        let out = merge(
            &[s("Chrome.exe"), s("svchost.exe"), s("chrome.exe")],
            &[(s("chrome.exe"), s("Google Chrome")), (s("CHROME.EXE"), s("Other Name")), (s("code.exe"), s("Visual Studio Code"))],
        );
        let find = |exe: &str| out.iter().find(|c| c.exe == exe).unwrap();
        assert_eq!(out.iter().filter(|c| c.exe == "chrome.exe").count(), 1, "no duplicates");
        assert_eq!(find("chrome.exe").name, "Google Chrome", "the first friendly name wins");
        assert!(find("chrome.exe").running && find("chrome.exe").installed);
        assert!(!find("code.exe").running && find("code.exe").installed);
        assert_eq!(find("svchost.exe").name, "svchost", "running-only apps fall back to the file name");
        assert!(find("svchost.exe").running && !find("svchost.exe").installed);
    }

    #[test]
    fn merge_only_keeps_names_the_guard_accepts() {
        let long = format!("{}.exe", "a".repeat(80));
        let out = merge(
            &[s("ok.exe"), s("C:\\evil\\path.exe"), s("no-extension"), s(""), s("tab\tname.exe"), long, s("a:b.exe")],
            &[(s("fine.exe"), s("Fine")), (s("bad/name.exe"), s("Bad"))],
        );
        let exes: Vec<&str> = out.iter().map(|c| c.exe.as_str()).collect();
        assert_eq!(exes, ["fine.exe", "ok.exe"], "{exes:?}");
        for c in &out {
            assert!(ExeName::new(&c.exe).is_ok() && c.exe == c.exe.to_lowercase());
        }
    }

    #[test]
    fn installed_apps_come_first_then_running_only_each_sorted_by_name() {
        let out = merge(
            &[s("zzz.exe"), s("aaa.exe")],
            &[(s("b.exe"), s("Beta")), (s("a.exe"), s("alpha")), (s("c.exe"), s("Charlie"))],
        );
        let order: Vec<&str> = out.iter().map(|c| c.exe.as_str()).collect();
        assert_eq!(order, ["a.exe", "b.exe", "c.exe", "aaa.exe", "zzz.exe"]);
    }

    #[test]
    fn the_result_is_capped() {
        let running: Vec<String> = (0..MAX_CANDIDATES + 50).map(|i| format!("app{i}.exe")).collect();
        assert_eq!(merge(&running, &[]).len(), MAX_CANDIDATES);
    }
}
