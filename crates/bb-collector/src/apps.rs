//! Programas deste PC, para o usuário ESCOLHER em vez de digitar nomes (listas de privacidade).
//!
//! Fontes (todas locais): processos em execução, `App Paths` e a lista de programas instalados do registro.
//! O resultado só existe em memória: nada é gravado em disco, registrado em log ou enviado. Só entram nomes
//! que o Guard aceitaria (`ExeName`), sempre em minúsculas e terminados em `.exe`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bb_core::ExeName;

use crate::lnk;

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

/// Maior manifesto de pacote lido (um `AppxManifest.xml` real tem poucos KB).
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Executáveis declarados no manifesto de um app da Loja (`<Application Executable="app\Foo.exe" ...>`). Só o NOME do
/// arquivo é devolvido, em minúsculas e validado como o Guard faria; sem caminho, sem repetição. Não é um leitor de XML:
/// só procura o atributo `Executable` nas marcas `<Application ...>` (o manifesto não é confiável e nada dele é executado).
pub fn manifest_executables(xml: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<Application") {
        rest = &rest[i + "<Application".len()..];
        // `<Applications>`, `<ApplicationContentUriRules>` etc. não são a marca procurada: exige espaço depois do nome.
        if !rest.starts_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        let lower = tag.to_ascii_lowercase();
        let Some(at) = lower.find("executable=") else { continue };
        let value = tag[at + "executable=".len()..].trim_start();
        let Some(quote) = value.chars().next().filter(|q| *q == '"' || *q == '\'') else { continue };
        let Some(close) = value[1..].find(quote) else { continue };
        let name = value[1..1 + close].rsplit(['\\', '/']).next().unwrap_or("").trim().to_lowercase();
        if name.ends_with(".exe") && ExeName::new(&name).is_ok() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Nome para mostrar de um app da Loja: o `DisplayName` do pacote quando é um texto de verdade (nem vazio, nem uma
/// referência de recurso como `@{...}` ou `ms-resource:...`, que não dá para ler sem a API de recursos).
pub fn store_display_name(raw: &str) -> Option<String> {
    let n = raw.trim();
    let bad = n.is_empty() || n.starts_with('@') || n.to_ascii_lowercase().starts_with("ms-resource:") || n.len() > 120;
    (!bad).then(|| n.to_owned())
}

/// Os apps de um pacote da Loja: (exe, nome amigável) para cada executável do manifesto.
pub fn store_apps(display_name: &str, manifest: &str) -> Vec<(String, String)> {
    let friendly = store_display_name(display_name);
    manifest_executables(manifest)
        .into_iter()
        .map(|exe| {
            let name = friendly.clone().unwrap_or_else(|| stem(&exe));
            (exe, name)
        })
        .collect()
}

/// Quantos níveis de pastas do Menu Iniciar são percorridos e quantos atalhos são lidos no máximo.
const MAX_SHORTCUT_DEPTH: usize = 6;
const MAX_SHORTCUT_FILES: usize = 5000;

/// Programas genéricos que muitos atalhos abrem com um destino diferente (um painel, uma pasta, um script). O nome do
/// atalho descreve o que ele abre, não o programa, então nesses casos o nome mostrado é o do próprio executável.
const GENERIC_HOSTS: &[&str] = &[
    "cmd.exe", "powershell.exe", "pwsh.exe", "explorer.exe", "control.exe", "mmc.exe", "rundll32.exe", "wscript.exe",
    "cscript.exe", "conhost.exe", "msiexec.exe", "regedit.exe", "mshta.exe", "wsl.exe", "bash.exe",
];

/// Atalhos que só servem para desinstalar ou consertar não são programas que o usuário queira escolher.
fn is_noise(exe: &str, shortcut_name: &str) -> bool {
    let stem = stem(exe);
    let name = shortcut_name.to_lowercase();
    exe == "msiexec.exe"
        || stem.contains("uninst")
        || stem.starts_with("unins0")
        || name.contains("uninstall")
        || name.contains("desinstal")
}

/// Programas dos atalhos (`.lnk`) sob `roots`: (nome do executável, nome do atalho). Só lê arquivos pequenos
/// `.lnk` e não segue links simbólicos; o destino de cada atalho é lido pelo leitor mínimo de `lnk` (sem COM, sem
/// resolver nada). Nenhum caminho sai daqui: só o nome do `.exe` e o nome que o atalho tem no Menu Iniciar.
pub fn shortcuts_in(roots: &[PathBuf]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut files_read = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if depth < MAX_SHORTCUT_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !is_lnk(&path) || files_read >= MAX_SHORTCUT_FILES {
                continue;
            }
            files_read += 1;
            if entry.metadata().map_or(true, |m| m.len() as usize > lnk::MAX_LNK_BYTES) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let (Some(exe), Some(name)) = (lnk::exe_of(&bytes), path.file_stem().map(|n| n.to_string_lossy().trim().to_owned())) else {
                continue;
            };
            if !name.is_empty() && !is_noise(&exe, &name) {
                let name = if GENERIC_HOSTS.contains(&exe.as_str()) { stem(&exe) } else { name };
                out.push((exe, name));
            }
        }
    }
    out.sort();
    out
}

fn is_lnk(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

#[cfg(windows)]
pub use win::{list_candidates, start_menu_programs, store_packages};

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

    use std::path::PathBuf;

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

    const PACKAGES: &str =
        "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";

    /// Apps da Loja (MSIX/AppX) instalados para este usuário, mesmo fora de execução: (exe, nome amigável). Lê a lista de
    /// pacotes do registro do usuário e o manifesto de cada um (arquivo pequeno, só leitura, sem rede e sem COM). Só pastas
    /// de `WindowsApps` são lidas e só o arquivo `AppxManifest.xml`; nenhum caminho sai daqui.
    pub fn store_packages() -> Vec<(String, String)> {
        let mut out = Vec::new();
        let Some(parent) = open(HKEY_CURRENT_USER, PACKAGES) else { return out };
        for sub in subkeys(&parent) {
            // Pacotes de recursos (idioma, escala) não têm aplicativo: pular poupa abrir centenas de arquivos.
            if sub.contains("_split.") || sub.contains("_~_") {
                continue;
            }
            let Some(entry) = open(HKEY_CURRENT_USER, &format!("{PACKAGES}\\{sub}")) else { continue };
            let Some(root) = read_string(&entry, "PackageRootFolder") else { continue };
            if !root.to_ascii_lowercase().contains("\\windowsapps\\") {
                continue;
            }
            let manifest = PathBuf::from(root).join("AppxManifest.xml");
            let Ok(meta) = std::fs::metadata(&manifest) else { continue };
            if meta.len() as usize > super::MAX_MANIFEST_BYTES {
                continue;
            }
            let Ok(xml) = std::fs::read_to_string(&manifest) else { continue };
            out.extend(super::store_apps(&read_string(&entry, "DisplayName").unwrap_or_default(), &xml));
        }
        out
    }

    /// Candidatos para as listas de privacidade. Rápido (milissegundos) e sem efeitos colaterais.
    /// Pastas de atalhos do Menu Iniciar (do usuário e de todos os usuários).
    fn start_menu_roots() -> Vec<PathBuf> {
        ["APPDATA", "PROGRAMDATA"]
            .iter()
            .filter_map(|v| std::env::var_os(v))
            .map(|base| PathBuf::from(base).join("Microsoft").join("Windows").join("Start Menu").join("Programs"))
            .collect()
    }

    pub fn start_menu_programs() -> Vec<(String, String)> {
        super::shortcuts_in(&start_menu_roots())
    }

    pub fn list_candidates() -> Vec<AppCandidate> {
        // Os nomes dos atalhos do Menu Iniciar vêm primeiro: são os que o usuário conhece ("Google Chrome").
        let mut installed = start_menu_programs();
        installed.extend(self::installed());
        installed.extend(store_packages());
        merge(&running_exes(), &installed)
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

    const MANIFEST: &str = r#"<?xml version="1.0"?><Package><Applications>
        <Application Id="App" Executable="WhatsApp.Root.exe" EntryPoint="Windows.FullTrustApplication">
        <uap:VisualElements/></Application>
        <Application Id="Helper" executable='bin\Synth Helper.EXE'/>
        <Application Id="NoExe" EntryPoint="x"/></Applications></Package>"#;

    #[test]
    fn the_manifest_gives_the_executables_of_a_store_app_without_paths() {
        assert_eq!(manifest_executables(MANIFEST), vec!["whatsapp.root.exe", "synth helper.exe"]);
    }

    #[test]
    fn the_manifest_reader_ignores_look_alikes_and_hostile_values() {
        for xml in [
            "",
            "<Applications Executable=\"a.exe\">",
            "<ApplicationContentUriRules Executable=\"a.exe\"/>",
            "<Application Executable=\"..\\..\\x\\readme.txt\"/>",
            "<Application Executable=\"con:*?.exe\"/>",
            "<Application Executable=a.exe/>",
            "<Application Executable=\"unterminated.exe",
            "<Application Executable=\"\"/>",
        ] {
            assert!(manifest_executables(xml).is_empty(), "{xml:?}");
        }
        assert_eq!(manifest_executables("<Application Executable=\"..\\..\\x\\evil.exe\"/>"), vec!["evil.exe"], "only the file name survives");
        let twice = "<Application Executable=\"a.exe\"/><Application Executable=\"A.exe\"/>";
        assert_eq!(manifest_executables(twice), vec!["a.exe"], "no repeats");
    }

    #[test]
    fn the_store_name_is_used_unless_it_is_an_unreadable_resource_reference() {
        assert_eq!(store_display_name("WhatsApp").as_deref(), Some("WhatsApp"));
        for bad in ["", "  ", "@{Synth?ms-resource://x/name}", "ms-resource:AppName", "MS-RESOURCE:x"] {
            assert_eq!(store_display_name(bad), None, "{bad:?}");
        }
        let apps = store_apps("@{x}", MANIFEST);
        assert_eq!(apps[0], ("whatsapp.root.exe".to_owned(), "whatsapp.root".to_owned()), "falls back to the file name");
        assert_eq!(store_apps("Synth Chat", MANIFEST)[0].1, "Synth Chat");
    }

    #[test]
    fn an_installed_store_app_that_is_not_running_is_listed_with_its_name() {
        let installed = store_apps("Synth Chat", MANIFEST);
        let list = merge(&[], &installed);
        let chat = list.iter().find(|c| c.exe == "whatsapp.root.exe").unwrap();
        assert!(chat.installed && !chat.running);
        assert_eq!(chat.name, "Synth Chat");
        // Em execução também: um só item, com o nome amigável.
        let both = merge(&["whatsapp.root.exe".to_owned()], &installed);
        assert_eq!(both.iter().filter(|c| c.exe == "whatsapp.root.exe").count(), 1);
        assert!(both.iter().find(|c| c.exe == "whatsapp.root.exe").unwrap().running);
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

    // ---- atalhos do Menu Iniciar ----

    use crate::lnk::fixtures::Lnk;
    use std::fs;

    fn write(dir: &std::path::Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    #[test]
    fn programs_are_read_from_shortcuts_in_nested_folders_with_the_shortcut_name() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "App.lnk", &Lnk::local("C:\\Program Files\\App\\app.exe").build());
        write(dir.path(), "Sub\\Deeper\\Tool.LNK", &Lnk::local("C:\\x\\Tool.exe").build());
        write(dir.path(), "My Game.lnk", &Lnk::local("D:\\Games\\game.exe").build());
        let found = shortcuts_in(&[dir.path().to_path_buf()]);
        assert_eq!(
            found,
            vec![(s("app.exe"), s("App")), (s("game.exe"), s("My Game")), (s("tool.exe"), s("Tool"))],
            "name = shortcut name, exe = lowercase file name, nothing else"
        );
    }

    #[test]
    fn uninstallers_documents_and_garbage_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Real.lnk", &Lnk::local("C:\\x\\real.exe").build());
        write(dir.path(), "Uninstall Real.lnk", &Lnk::local("C:\\x\\real.exe").build());
        write(dir.path(), "Remove.lnk", &Lnk::local("C:\\x\\uninstall.exe").build());
        write(dir.path(), "Remove2.lnk", &Lnk::local("C:\\x\\unins000.exe").build());
        write(dir.path(), "Installer.lnk", &Lnk::local("C:\\Windows\\System32\\msiexec.exe").build());
        write(dir.path(), "Readme.lnk", &Lnk::local("C:\\x\\readme.txt").build());
        write(dir.path(), "Broken.lnk", b"this is not a shortcut at all");
        write(dir.path(), "Empty.lnk", b"");
        write(dir.path(), "Huge.lnk", &vec![0x41u8; crate::lnk::MAX_LNK_BYTES + 10]);
        write(dir.path(), "notes.txt", &Lnk::local("C:\\x\\sneaky.exe").build()); // não é .lnk
        assert_eq!(shortcuts_in(&[dir.path().to_path_buf()]), vec![(s("real.exe"), s("Real"))]);
    }

    #[test]
    fn missing_folders_and_the_depth_limit_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        assert!(shortcuts_in(&[dir.path().join("does-not-exist")]).is_empty());
        let deep = (0..MAX_SHORTCUT_DEPTH + 3).map(|i| format!("d{i}")).collect::<Vec<_>>().join("\\");
        write(dir.path(), &format!("{deep}\\Too Deep.lnk"), &Lnk::local("C:\\x\\deep.exe").build());
        write(dir.path(), "d0\\d1\\Shallow.lnk", &Lnk::local("C:\\x\\shallow.exe").build());
        assert_eq!(shortcuts_in(&[dir.path().to_path_buf()]), vec![(s("shallow.exe"), s("Shallow"))]);
    }

    #[test]
    fn shortcuts_to_generic_hosts_show_the_executable_not_what_they_open() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Administrative Tools.lnk", &Lnk::local("C:\\Windows\\System32\\control.exe").build());
        write(dir.path(), "My Script.lnk", &Lnk::local("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe").build());
        write(dir.path(), "Real App.lnk", &Lnk::local("C:\\x\\realapp.exe").build());
        let found = shortcuts_in(&[dir.path().to_path_buf()]);
        assert_eq!(
            found,
            vec![(s("control.exe"), s("control")), (s("powershell.exe"), s("powershell")), (s("realapp.exe"), s("Real App"))]
        );
    }

    #[test]
    fn shortcut_names_win_in_the_merged_list() {
        let shortcuts = vec![(s("chrome.exe"), s("Google Chrome"))];
        let out = merge(&[s("chrome.exe")], &shortcuts);
        assert_eq!(out[0].name, "Google Chrome");
        assert!(out[0].installed && out[0].running);
    }

    #[test]
    fn a_shortcut_to_an_invalid_name_never_reaches_the_list() {
        let long = format!("C:\\x\\{}.exe", "a".repeat(80));
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Long.lnk", &Lnk::local(&long).build());
        let found = shortcuts_in(&[dir.path().to_path_buf()]);
        // O leitor devolve o nome; o filtro do Guard (ExeName) o descarta ao montar a lista.
        assert!(merge(&[], &found).is_empty());
    }
}
