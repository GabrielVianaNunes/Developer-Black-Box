//! Verificação de atualizações do Developer Black Box.
//!
//! É a ÚNICA parte do app que fala com a rede, e faz o mínimo possível:
//!   * uma requisição `GET` HTTPS para o endereço fixo da última Release do projeto no GitHub;
//!   * sem corpo, sem cookies, sem conta e sem dado seu ou do seu computador. Os únicos cabeçalhos
//!     são os exigidos pelo GitHub (`Accept`, `X-GitHub-Api-Version`) e `User-Agent: DeveloperBlackBox/<versão>`;
//!   * a resposta é só lida: nada é baixado, executado ou instalado. O link mostrado ao usuário é montado
//!     aqui a partir da versão validada, nunca copiado da resposta.
//!
//! A rede usa o WinHTTP do Windows (TLS do sistema); nenhuma biblioteca HTTP/TLS de terceiros entra no app.

mod version;
#[cfg(windows)]
mod winhttp;

pub use version::{Channel, Version};
#[cfg(windows)]
pub use winhttp::WinHttpFetcher;

use serde::Deserialize;

pub const HOST: &str = "api.github.com";
pub const OWNER: &str = "GabrielVianaNunes";
pub const REPO: &str = "Developer-Black-Box";
/// Respostas maiores que isto são recusadas (a de uma Release tem poucos KB).
pub const MAX_BODY: usize = 512 * 1024;

/// Caminho da API da última Release estável (o GitHub ignora rascunhos e pré-lançamentos).
pub fn latest_path() -> String {
    format!("/repos/{OWNER}/{REPO}/releases/latest")
}

/// Página pública da Release de uma versão. Montada só a partir de uma versão já validada.
pub fn release_url(version: &Version) -> String {
    format!("https://github.com/{OWNER}/{REPO}/releases/tag/v{version}")
}

/// Por que a verificação falhou. `code()` é um código neutro de idioma que a interface traduz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// Sem conexão, DNS, TLS ou tempo esgotado.
    Network,
    /// O GitHub respondeu com outro status (limite de uso, indisponível etc.).
    Status(u16),
    /// Ainda não existe nenhuma Release publicada.
    NoRelease,
    /// A resposta não tem a forma esperada ou a versão é inválida.
    BadResponse,
    /// A resposta passou do limite de tamanho.
    TooLarge,
}

impl UpdateError {
    pub fn code(&self) -> &'static str {
        match self {
            UpdateError::Network => "update.network",
            UpdateError::Status(_) => "update.status",
            UpdateError::NoRelease => "update.no_release",
            UpdateError::BadResponse => "update.bad_response",
            UpdateError::TooLarge => "update.too_large",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Available { version: Version },
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Transporte HTTPS. Existe para que a lógica seja testada sem rede.
pub trait Fetcher {
    /// `GET https://{host}{path}` e devolve status e corpo (no máximo `MAX_BODY` bytes).
    fn get(&self, host: &str, path: &str) -> Result<Response, UpdateError>;
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Interpreta a resposta da API. Só a tag e os indicadores de rascunho/pré-lançamento são usados.
pub fn parse_latest(body: &[u8]) -> Result<Option<Version>, UpdateError> {
    if body.len() > MAX_BODY {
        return Err(UpdateError::TooLarge);
    }
    let rel: ReleaseJson = serde_json::from_slice(body).map_err(|_| UpdateError::BadResponse)?;
    let version = Version::from_tag(&rel.tag_name).ok_or(UpdateError::BadResponse)?;
    // Rascunhos e pré-lançamentos nunca são oferecidos como atualização.
    if rel.draft || rel.prerelease || version.is_prerelease() {
        return Ok(None);
    }
    Ok(Some(version))
}

/// Compara a versão instalada com a última Release publicada.
pub fn check(current: &Version, fetcher: &dyn Fetcher) -> Result<Outcome, UpdateError> {
    let resp = fetcher.get(HOST, &latest_path())?;
    match resp.status {
        200 => {}
        404 => return Err(UpdateError::NoRelease),
        other => return Err(UpdateError::Status(other)),
    }
    match parse_latest(&resp.body)? {
        Some(latest) if latest > *current => Ok(Outcome::Available { version: latest }),
        _ => Ok(Outcome::UpToDate),
    }
}

/// Versão instalada (a do `Cargo.toml` do workspace).
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace version is valid semver (checked by scripts/version.mjs)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        reply: Result<(u16, Vec<u8>), UpdateError>,
        asked: RefCell<Vec<(String, String)>>,
    }

    impl Fake {
        fn ok(status: u16, body: &str) -> Fake {
            Fake { reply: Ok((status, body.as_bytes().to_vec())), asked: RefCell::new(vec![]) }
        }
    }

    impl Fetcher for Fake {
        fn get(&self, host: &str, path: &str) -> Result<Response, UpdateError> {
            self.asked.borrow_mut().push((host.into(), path.into()));
            self.reply.clone().map(|(status, body)| Response { status, body })
        }
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    const NEWER: &str = r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false,"html_url":"https://evil.example/x","body":"notes"}"#;

    #[test]
    fn reports_a_newer_release() {
        let f = Fake::ok(200, NEWER);
        assert_eq!(check(&v("0.1.0"), &f), Ok(Outcome::Available { version: v("0.2.0") }));
    }

    #[test]
    fn same_and_older_releases_are_not_updates() {
        assert_eq!(check(&v("0.2.0"), &Fake::ok(200, NEWER)), Ok(Outcome::UpToDate));
        assert_eq!(check(&v("0.3.0"), &Fake::ok(200, NEWER)), Ok(Outcome::UpToDate), "never suggests a downgrade");
    }

    #[test]
    fn drafts_and_prereleases_are_never_offered() {
        for body in [
            r#"{"tag_name":"v9.0.0","draft":true}"#,
            r#"{"tag_name":"v9.0.0","prerelease":true}"#,
            r#"{"tag_name":"v9.0.0-rc.1"}"#,
        ] {
            assert_eq!(check(&v("0.1.0"), &Fake::ok(200, body)), Ok(Outcome::UpToDate), "{body}");
        }
    }

    #[test]
    fn a_prerelease_install_is_offered_the_final_version() {
        let f = Fake::ok(200, r#"{"tag_name":"v1.0.0"}"#);
        assert_eq!(check(&v("1.0.0-rc.1"), &f), Ok(Outcome::Available { version: v("1.0.0") }));
    }

    #[test]
    fn bad_responses_are_errors_not_updates() {
        for body in ["", "not json", "{}", r#"{"tag_name":7}"#, r#"{"tag_name":"0.2.0"}"#, r#"{"tag_name":"v0.2"}"#, r#"{"tag_name":"latest"}"#] {
            assert_eq!(check(&v("0.1.0"), &Fake::ok(200, body)), Err(UpdateError::BadResponse), "{body:?}");
        }
    }

    #[test]
    fn statuses_map_to_distinct_errors() {
        assert_eq!(check(&v("0.1.0"), &Fake::ok(404, "{}")), Err(UpdateError::NoRelease));
        assert_eq!(check(&v("0.1.0"), &Fake::ok(403, "{}")), Err(UpdateError::Status(403)));
        assert_eq!(check(&v("0.1.0"), &Fake::ok(500, "")), Err(UpdateError::Status(500)));
        assert_eq!(check(&v("0.1.0"), &Fake::ok(204, "")), Err(UpdateError::Status(204)), "only 200 carries a release");
        let down = Fake { reply: Err(UpdateError::Network), asked: RefCell::new(vec![]) };
        assert_eq!(check(&v("0.1.0"), &down), Err(UpdateError::Network));
    }

    #[test]
    fn oversized_responses_are_refused() {
        let big = format!(r#"{{"tag_name":"v0.2.0","body":"{}"}}"#, "x".repeat(MAX_BODY));
        assert_eq!(check(&v("0.1.0"), &Fake::ok(200, &big)), Err(UpdateError::TooLarge));
    }

    #[test]
    fn asks_only_the_fixed_github_endpoint() {
        let f = Fake::ok(200, NEWER);
        check(&v("0.1.0"), &f).unwrap();
        let asked = f.asked.borrow();
        assert_eq!(
            asked.as_slice(),
            [("api.github.com".to_string(), "/repos/GabrielVianaNunes/Developer-Black-Box/releases/latest".to_string())]
        );
    }

    #[test]
    fn the_link_comes_from_the_validated_version_never_from_the_response() {
        // A resposta traz um `html_url` malicioso; ele é ignorado.
        let Ok(Outcome::Available { version }) = check(&v("0.1.0"), &Fake::ok(200, NEWER)) else { panic!() };
        let url = release_url(&version);
        assert_eq!(url, "https://github.com/GabrielVianaNunes/Developer-Black-Box/releases/tag/v0.2.0");
        assert!(!url.contains("evil"));
    }

    #[test]
    fn error_codes_are_neutral_and_distinct() {
        let all = [UpdateError::Network, UpdateError::Status(500), UpdateError::NoRelease, UpdateError::BadResponse, UpdateError::TooLarge];
        let codes: std::collections::BTreeSet<_> = all.iter().map(UpdateError::code).collect();
        assert_eq!(codes.len(), all.len());
        assert!(codes.iter().all(|c| c.starts_with("update.") && c.is_ascii()));
    }

    #[test]
    fn the_installed_version_is_valid_semver() {
        let _ = current_version();
    }
}
