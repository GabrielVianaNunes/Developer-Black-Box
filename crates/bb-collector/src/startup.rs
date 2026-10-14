//! Início automático com o Windows, pela chave `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//!
//! Só o registro do usuário atual: não exige administrador e não mexe em nada do sistema.
//! Iniciar com o Windows NÃO inicia a gravação: quem decide isso é a configuração de gravação.

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};

use crate::sample::CollectError;

/// Nome do valor usado pelo app.
pub const APP_VALUE_NAME: &str = "DeveloperBlackBox";

/// Chave de programas que iniciam com o Windows, no registro do usuário atual.
pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

pub struct StartupEntry {
    subkey: String,
    value_name: String,
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

/// Abre a chave. `create` cria a chave se ela não existir (num Windows novo a chave `Run` pode não existir
/// ainda); sem `create`, uma chave ausente devolve `None`: não há nada registrado.
fn open_key(subkey: &str, create: bool) -> Result<Option<Key>, CollectError> {
    let path = wide(subkey);
    let mut h = HKEY::default();
    let access = KEY_SET_VALUE | KEY_QUERY_VALUE;
    // SAFETY: `path` é terminado em NUL e vive durante a chamada; `h` recebe um handle válido se houver êxito.
    let status = unsafe {
        if create {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                access,
                None,
                &mut h,
                None,
            )
        } else {
            RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, access, &mut h)
        }
    };
    if !create && status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(|e| CollectError(format!("open Run key: {e}")))?;
    Ok(Some(Key(h)))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl StartupEntry {
    pub fn new(value_name: &str) -> Self {
        Self { subkey: RUN_KEY.to_owned(), value_name: value_name.to_owned() }
    }

    /// Como `new`, mas numa subchave de `HKEY_CURRENT_USER` à escolha (usado em testes, para exercitar
    /// uma chave que ainda não existe sem tocar na chave `Run` real).
    pub fn in_key(subkey: &str, value_name: &str) -> Self {
        Self { subkey: subkey.to_owned(), value_name: value_name.to_owned() }
    }

    /// A entrada do próprio aplicativo.
    pub fn app() -> Self {
        Self::new(APP_VALUE_NAME)
    }

    /// Registra `"<exe>" <args>` para iniciar no login do usuário. O caminho precisa ser absoluto
    /// e sem aspas, para o comando não poder ser distorcido.
    pub fn enable(&self, exe: &Path, args: &str) -> Result<(), CollectError> {
        let exe_str = exe.to_str().ok_or_else(|| CollectError("executable path is not valid text".into()))?;
        if !exe.is_absolute() || exe_str.contains('"') {
            return Err(CollectError("executable path must be absolute and free of quotes".into()));
        }
        let command = if args.is_empty() { format!("\"{exe_str}\"") } else { format!("\"{exe_str}\" {args}") };
        let data: Vec<u8> = wide(&command).iter().flat_map(|c| c.to_le_bytes()).collect();
        let name = wide(&self.value_name);
        let key = open_key(&self.subkey, true)?.ok_or_else(|| CollectError("open Run key: not created".into()))?;
        // SAFETY: `name` e `data` são terminados em NUL e vivem durante a chamada.
        unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)) }
            .ok()
            .map_err(|e| CollectError(format!("write Run value: {e}")))
    }

    /// Remove a entrada. Não é erro se ela já não existir.
    pub fn disable(&self) -> Result<(), CollectError> {
        let name = wide(&self.value_name);
        // Chave ausente: não há entrada a remover.
        let Some(key) = open_key(&self.subkey, false)? else { return Ok(()) };
        // SAFETY: `name` é terminado em NUL.
        let status = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status.ok().map_err(|e| CollectError(format!("delete Run value: {e}")))
    }

    /// Comando registrado, se houver.
    pub fn command(&self) -> Option<String> {
        let name = wide(&self.value_name);
        let key = open_key(&self.subkey, false).ok()??;
        let mut kind = REG_VALUE_TYPE::default();
        let mut len = 0u32;
        // SAFETY: primeira chamada só pergunta o tamanho.
        unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, Some(&mut kind), None, Some(&mut len)) }.ok().ok()?;
        if kind != REG_SZ || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` tem `len` bytes.
        unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, Some(&mut kind), Some(buf.as_mut_ptr()), Some(&mut len)) }
            .ok()
            .ok()?;
        let units: Vec<u16> = buf[..len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        Some(String::from_utf16_lossy(&units[..end]))
    }

    pub fn is_enabled(&self) -> bool {
        self.command().is_some()
    }
}
