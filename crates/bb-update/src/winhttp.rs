//! Transporte HTTPS sobre o WinHTTP do Windows (TLS do sistema, certificados do sistema, proxy do sistema).
//!
//! Sem cookies e sem autenticação automática; só TLS 1.2 ou 1.3; tempos limite curtos; o corpo é lido
//! em pedaços e abortado ao passar de `MAX_BODY`. Os redirecionamentos seguem a regra padrão do WinHTTP
//! (nunca de HTTPS para HTTP).

use std::ffi::c_void;
use std::ptr::null_mut;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES,
    WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

use crate::{Fetcher, Response, UpdateError, MAX_BODY};

const TIMEOUT_MS: i32 = 10_000;
const HEADERS: &str = "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n";

/// Fecha o identificador do WinHTTP ao sair de escopo, em qualquer caminho de erro.
struct Handle(*mut c_void);

impl Handle {
    fn open(raw: *mut c_void) -> Result<Handle, UpdateError> {
        if raw.is_null() { Err(UpdateError::Network) } else { Ok(Handle(raw)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: o identificador veio de uma função do WinHTTP e é fechado uma única vez.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct WinHttpFetcher {
    user_agent: String,
}

impl WinHttpFetcher {
    /// `version` vai no `User-Agent: DeveloperBlackBox/<versão>`, que o GitHub exige. Nada mais identifica você.
    pub fn new(version: &str) -> Self {
        WinHttpFetcher { user_agent: format!("DeveloperBlackBox/{version}") }
    }
}

impl Fetcher for WinHttpFetcher {
    fn get(&self, host: &str, path: &str) -> Result<Response, UpdateError> {
        let (agent, host_w, path_w) = (wide(&self.user_agent), wide(host), wide(path));
        let headers: Vec<u16> = HEADERS.encode_utf16().collect();
        // SAFETY: todas as cadeias são UTF-16 terminadas em zero e vivem até o fim da função; os
        // identificadores são fechados por `Handle`; os buffers passados têm o tamanho informado.
        unsafe {
            let session = Handle::open(WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ))?;
            WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS).map_err(|_| UpdateError::Network)?;
            let protocols = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3).to_ne_bytes();
            WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&protocols))
                .map_err(|_| UpdateError::Network)?;

            let connection = Handle::open(WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
            let request = Handle::open(WinHttpOpenRequest(
                connection.0,
                PCWSTR(wide("GET").as_ptr()),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ))?;
            let disabled = (WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION).to_ne_bytes();
            WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&disabled)).map_err(|_| UpdateError::Network)?;

            WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).map_err(|_| UpdateError::Network)?;
            WinHttpReceiveResponse(request.0, null_mut()).map_err(|_| UpdateError::Network)?;

            let mut status: u32 = 0;
            let mut len = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut c_void),
                &mut len,
                null_mut(),
            )
            .map_err(|_| UpdateError::BadResponse)?;

            let mut body = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)
                    .map_err(|_| UpdateError::Network)?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                if body.len() > MAX_BODY {
                    return Err(UpdateError::TooLarge);
                }
            }
            Ok(Response { status: status as u16, body })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, current_version, Outcome};

    /// Consulta REAL ao GitHub. Fica de fora da suíte normal (precisa de internet):
    ///   cargo test -p bb-update -- --ignored --nocapture
    #[test]
    #[ignore = "usa a internet"]
    fn live_check_against_github() {
        let fetcher = WinHttpFetcher::new(&current_version().to_string());
        let outcome = check(&current_version(), &fetcher);
        println!("live outcome: {outcome:?}");
        assert!(matches!(outcome, Ok(Outcome::UpToDate) | Ok(Outcome::Available { .. })), "{outcome:?}");
        // Com uma versão instalada antiga, a Release real tem de aparecer como atualização.
        let old = crate::Version::parse("0.0.1").unwrap();
        let outcome = check(&old, &fetcher);
        println!("live outcome (installed 0.0.1): {outcome:?}");
        assert!(matches!(outcome, Ok(Outcome::Available { .. })), "{outcome:?}");
    }

    /// Falha de rede vira erro, não pânico nem "atualização".
    #[test]
    fn an_unreachable_host_is_a_network_error() {
        let fetcher = WinHttpFetcher::new("0.0.0");
        // `.invalid` nunca resolve (RFC 2606).
        assert_eq!(fetcher.get("github.invalid", "/").err(), Some(UpdateError::Network));
    }
}
