//! Origem da chave mestra de 256 bits.
//!
//! No Windows a chave fica protegida por DPAPI (vinculada à conta do usuário),
//! então o arquivo em disco sozinho não permite decifrar as evidências.

use crate::error::Result;

pub trait KeyProvider {
    fn key(&self) -> Result<[u8; 32]>;
}

/// Chave fixa em memória, para testes com dados sintéticos.
pub struct StaticKey(pub [u8; 32]);

impl KeyProvider for StaticKey {
    fn key(&self) -> Result<[u8; 32]> {
        Ok(self.0)
    }
}

#[cfg(windows)]
pub use dpapi::DpapiKeyStore;

#[cfg(windows)]
mod dpapi {
    use std::path::{Path, PathBuf};

    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

    use super::KeyProvider;
    use crate::error::{RecorderError, Result};

    /// Entropia fixa do aplicativo: separa este uso de outros blobs DPAPI da conta.
    const ENTROPY: &[u8] = b"DeveloperBlackBox/master-key/v1";

    pub struct DpapiKeyStore {
        path: PathBuf,
    }

    impl DpapiKeyStore {
        pub fn new(path: impl AsRef<Path>) -> Self {
            Self { path: path.as_ref().to_path_buf() }
        }
    }

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: o Windows aloca `out.pbData` com `cbData` bytes; copiamos e liberamos.
        unsafe {
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
            v
        }
    }

    fn protect(plain: &[u8]) -> Result<Vec<u8>> {
        let input = blob(plain);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: ponteiros válidos durante a chamada.
        unsafe {
            CryptProtectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| RecorderError::KeyStore(format!("CryptProtectData: {e}")))?;
        }
        Ok(take(out))
    }

    fn unprotect(wrapped: &[u8]) -> Result<Vec<u8>> {
        let input = blob(wrapped);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: ponteiros válidos durante a chamada.
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| RecorderError::KeyStore(format!("CryptUnprotectData: {e}")))?;
        }
        Ok(take(out))
    }

    impl KeyProvider for DpapiKeyStore {
        /// Lê a chave protegida ou gera uma nova na primeira execução.
        fn key(&self) -> Result<[u8; 32]> {
            if self.path.exists() {
                let plain = unprotect(&std::fs::read(&self.path)?)?;
                return <[u8; 32]>::try_from(plain.as_slice()).map_err(|_| RecorderError::KeyStore("unexpected key length".into()));
            }
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&self.path, protect(&key)?)?;
            Ok(key)
        }
    }
}
