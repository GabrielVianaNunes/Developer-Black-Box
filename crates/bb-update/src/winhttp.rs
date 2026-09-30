//! Transporte HTTPS sobre o WinHTTP do Windows (TLS do sistema, certificados do sistema, proxy do sistema).
//!
//! Sem cookies e sem autenticação automática; só TLS 1.2 ou 1.3; tempos limite curtos; o corpo só é lido
//! se o status for 200 e é abortado ao passar do limite pedido. Os redirecionamentos seguem a regra padrão
//! do WinHTTP (nunca de HTTPS para HTTP). Nada do que é baixado é confiável por vir de um redirecionamento:
//! o conteúdo só vale depois de passar pela verificação de assinatura (`verify`).

use std::ffi::c_void;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::ptr::null_mut;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES,
    WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use crate::{Downloader, Fetcher, Response, UpdateError, MAX_BODY};

const TIMEOUT_MS: i32 = 10_000;
/// Em downloads grandes cada leitura pode demorar mais; o total é limitado pelo tamanho máximo.
const DOWNLOAD_RECEIVE_TIMEOUT_MS: i32 = 30_000;
const API_HEADERS: &str = "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n";
const DOWNLOAD_HEADERS: &str = "Accept: application/octet-stream\r\n";

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

/// Destino alternativo (só existe em builds com a feature `e2e`): um servidor local sem TLS.
#[cfg(feature = "e2e")]
#[derive(Clone)]
struct LocalOverride {
    host: String,
    port: u16,
}

pub struct WinHttpFetcher {
    user_agent: String,
    #[cfg(feature = "e2e")]
    local: Option<LocalOverride>,
}

impl WinHttpFetcher {
    /// `version` vai no `User-Agent: DeveloperBlackBox/<versão>`, que o GitHub exige. Nada mais identifica você.
    pub fn new(version: &str) -> Self {
        WinHttpFetcher {
            user_agent: format!("DeveloperBlackBox/{version}"),
            #[cfg(feature = "e2e")]
            local: None,
        }
    }

    /// SÓ PARA TESTE (feature `e2e`): manda tudo para `http://host:port` em vez do GitHub.
    #[cfg(feature = "e2e")]
    pub fn with_local_server(mut self, host: &str, port: u16) -> Self {
        self.local = Some(LocalOverride { host: host.into(), port });
        self
    }

    /// Uma requisição GET. O corpo só é entregue a `sink` se o status for 200.
    fn request(
        &self,
        host: &str,
        path: &str,
        headers: &str,
        max: u64,
        receive_timeout_ms: i32,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), UpdateError>,
    ) -> Result<u16, UpdateError> {
        #[allow(unused_mut)]
        let (mut host, mut port, mut secure) = (host.to_owned(), INTERNET_DEFAULT_HTTPS_PORT, true);
        #[cfg(feature = "e2e")]
        if let Some(l) = &self.local {
            (host, port, secure) = (l.host.clone(), l.port, false);
        }
        let (agent, host_w, path_w) = (wide(&self.user_agent), wide(&host), wide(path));
        let headers: Vec<u16> = headers.encode_utf16().collect();
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
            WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, receive_timeout_ms).map_err(|_| UpdateError::Network)?;
            let protocols = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3).to_ne_bytes();
            WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&protocols)).map_err(|_| UpdateError::Network)?;

            let connection = Handle::open(WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0))?;
            let request = Handle::open(WinHttpOpenRequest(
                connection.0,
                PCWSTR(wide("GET").as_ptr()),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) },
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
            if status != 200 {
                return Ok(status as u16);
            }

            let mut total: u64 = 0;
            let mut chunk = [0u8; 16 * 1024];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)
                    .map_err(|_| UpdateError::Network)?;
                if read == 0 {
                    break;
                }
                total += read as u64;
                if total > max {
                    return Err(UpdateError::TooLarge);
                }
                sink(&chunk[..read as usize])?;
            }
            Ok(200)
        }
    }
}

impl Fetcher for WinHttpFetcher {
    fn get(&self, host: &str, path: &str) -> Result<Response, UpdateError> {
        let mut body = Vec::new();
        let status = self.request(host, path, API_HEADERS, MAX_BODY as u64, TIMEOUT_MS, &mut |c| {
            body.extend_from_slice(c);
            Ok(())
        })?;
        Ok(Response { status, body })
    }
}

impl Downloader for WinHttpFetcher {
    fn download(&self, host: &str, path: &str, dest: &Path, max: u64) -> Result<u16, UpdateError> {
        let mut file = File::create(dest).map_err(|_| UpdateError::Disk)?;
        let status = self.request(host, path, DOWNLOAD_HEADERS, max, DOWNLOAD_RECEIVE_TIMEOUT_MS, &mut |c| {
            file.write_all(c).map_err(|_| UpdateError::Disk)
        })?;
        file.flush().map_err(|_| UpdateError::Disk)?;
        Ok(status)
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

    /// Download REAL do instalador publicado (v0.1.0) seguindo o redirecionamento do GitHub.
    ///   cargo test -p bb-update -- --ignored --nocapture
    #[test]
    #[ignore = "usa a internet"]
    fn live_download_of_the_published_installer_follows_redirects() {
        let fetcher = WinHttpFetcher::new("0.0.0");
        let dir = tempfile_dir();
        let dest = dir.join("setup.exe");
        let path = format!("/{}/{}/releases/download/v0.1.0/Developer-Black-Box_0.1.0_x64-setup.exe", crate::OWNER, crate::REPO);
        let status = fetcher.download("github.com", &path, &dest, 64 * 1024 * 1024).unwrap();
        let size = std::fs::metadata(&dest).unwrap().len();
        println!("live download: status {status}, {size} bytes");
        assert_eq!(status, 200);
        assert!(size > 1_000_000, "{size}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bb-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Falha de rede vira erro, não pânico nem "atualização".
    #[test]
    fn an_unreachable_host_is_a_network_error() {
        let fetcher = WinHttpFetcher::new("0.0.0");
        // `.invalid` nunca resolve (RFC 2606).
        assert_eq!(fetcher.get("github.invalid", "/").err(), Some(UpdateError::Network));
    }

    #[test]
    fn a_failed_download_does_not_leave_a_usable_file_promise() {
        let dir = tempfile_dir();
        let dest = dir.join("x.partial");
        let r = WinHttpFetcher::new("0.0.0").download("github.invalid", "/x", &dest, 1024);
        assert_eq!(r, Err(UpdateError::Network));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
