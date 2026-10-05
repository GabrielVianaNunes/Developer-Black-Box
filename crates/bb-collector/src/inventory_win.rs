//! Leitura do inventário no Windows, sem administrador.
//!
//! - BIOS (versão e data), Secure Boot e build do Windows: valores do registro (HKLM, leitura liberada a usuários comuns).
//! - Tipo de firmware: `GetFirmwareType`.
//! - Dispositivos com problema de driver: só o CÓDIGO de problema de cada dispositivo presente (`CM_Get_DevNode_Status`);
//!   nome, ID de instância e demais propriedades do dispositivo nunca são lidos.
//!
//! A decisão (empacotar, validar, resumir) está em `inventory` (puro). Cada fonte que falhar vira `None`, sem erro.

use windows::core::{w, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, CM_DEVNODE_STATUS_FLAGS, CM_PROB,
    CR_SUCCESS, DIGCF_ALLCLASSES, DIGCF_PRESENT, DN_HAS_PROBLEM, SP_DEVINFO_DATA,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, REG_DWORD, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::SystemInformation::{GetFirmwareType, FIRMWARE_TYPE};

use crate::inventory::{snapshot, InventorySource, Snapshot};
use crate::sample::CollectError;

/// Teto de dispositivos examinados por leitura (defesa contra uma enumeração anormal).
const MAX_DEVICES: u32 = 20_000;

pub struct WindowsInventorySource;

impl WindowsInventorySource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsInventorySource {
    fn default() -> Self {
        Self::new()
    }
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

fn open(subkey: PCWSTR) -> Option<Key> {
    let mut h = HKEY::default();
    // SAFETY: `subkey` é uma string literal terminada em NUL; `h` recebe um handle válido se houver êxito.
    unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, None, KEY_QUERY_VALUE, &mut h).ok().ok()? };
    Some(Key(h))
}

/// Lê um valor do registro como texto (REG_SZ) ou número (REG_DWORD); o tipo errado vira `None`.
fn read_value(key: &Key, name: PCWSTR) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
    let mut kind = REG_VALUE_TYPE::default();
    let mut len = 0u32;
    // SAFETY: duas chamadas: a primeira descobre o tamanho, a segunda preenche um buffer desse tamanho.
    unsafe {
        RegQueryValueExW(key.0, name, None, Some(&mut kind), None, Some(&mut len)).ok().ok()?;
        if len == 0 || len > 4096 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        RegQueryValueExW(key.0, name, None, Some(&mut kind), Some(buf.as_mut_ptr()), Some(&mut len)).ok().ok()?;
        buf.truncate(len as usize);
        Some((kind, buf))
    }
}

fn read_string(key: &Key, name: PCWSTR) -> Option<String> {
    let (kind, buf) = read_value(key, name)?;
    if kind != REG_SZ {
        return None;
    }
    let units: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    Some(String::from_utf16_lossy(&units))
}

fn read_dword(key: &Key, name: PCWSTR) -> Option<u32> {
    let (kind, buf) = read_value(key, name)?;
    (kind == REG_DWORD && buf.len() >= 4).then(|| u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]))
}

/// Códigos de problema dos dispositivos presentes. `None` se a enumeração não pôde ser feita.
fn device_problem_codes() -> Option<Vec<u32>> {
    let mut codes = Vec::new();
    // SAFETY: a lista de dispositivos é destruída ao final; `SP_DEVINFO_DATA` tem `cbSize` preenchido; nada é lido
    // além do código de problema.
    unsafe {
        let set = SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT).ok()?;
        for index in 0..MAX_DEVICES {
            let mut data = SP_DEVINFO_DATA { cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInfo(set, index, &mut data).is_err() {
                break; // fim da lista (ou erro: fica com o que já leu)
            }
            let mut status = CM_DEVNODE_STATUS_FLAGS::default();
            let mut problem = CM_PROB::default();
            if CM_Get_DevNode_Status(&mut status, &mut problem, data.DevInst, 0) == CR_SUCCESS && (status.0 & DN_HAS_PROBLEM.0) != 0 {
                codes.push(problem.0);
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    Some(codes)
}

impl InventorySource for WindowsInventorySource {
    fn read(&mut self) -> Result<Snapshot, CollectError> {
        let bios = open(w!("HARDWARE\\DESCRIPTION\\System\\BIOS"));
        let bios_version = bios.as_ref().and_then(|k| read_string(k, w!("BIOSVersion")));
        let bios_date = bios.as_ref().and_then(|k| read_string(k, w!("BIOSReleaseDate")));

        let mut fw = FIRMWARE_TYPE::default();
        // SAFETY: `fw` é um destino válido.
        let firmware = unsafe { GetFirmwareType(&mut fw).ok().map(|_| u64::from(fw.0 as u32)) };

        let secure_boot = open(w!("SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State"))
            .and_then(|k| read_dword(&k, w!("UEFISecureBootEnabled")))
            .map(|v| v != 0);

        let os = open(w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")).and_then(|k| {
            let build = read_string(&k, w!("CurrentBuildNumber"))?.trim().parse::<u32>().ok()?;
            Some((build, read_dword(&k, w!("UBR")).unwrap_or(0)))
        });

        let problems = device_problem_codes();
        Ok(snapshot(bios_version.as_deref(), bios_date.as_deref(), firmware, secure_boot, os, problems.as_deref()))
    }
}
