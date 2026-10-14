//! Proteção opcional do arquivo de exportação por uma senha escolhida na hora.
//!
//! Só primitivas padrão, sem algoritmo inventado: a chave de 256 bits sai da senha por Argon2id (sal aleatório de 16 bytes) e o
//! conteúdo é cifrado e autenticado por AES-256-GCM (nonce aleatório de 12 bytes). Os parâmetros do Argon2id e o formato vão
//! no cabeçalho do arquivo e entram como dado associado (AAD) da cifra: mexer em qualquer um deles faz a autenticação falhar.
//!
//! A senha NUNCA é gravada nem recuperável: senha perdida = arquivo perdido. Este módulo não guarda a senha, não a registra e
//! apaga a chave derivada da memória assim que termina de usá-la (melhor esforço: a linguagem não garante apagar cópias).

use aes_gcm::aead::{rand_core::RngCore, Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};

/// Nome do formato do arquivo protegido (o do conteúdo continua dentro, cifrado).
pub const FORMAT: &str = "developer-blackbox-export-protected/1";
const KDF_NAME: &str = "argon2id";

/// Menor senha aceita, em caracteres.
pub const MIN_PASSWORD_CHARS: usize = 8;
/// Maior senha aceita, em bytes (UTF-8).
pub const MAX_PASSWORD_BYTES: usize = 256;

/// Maior arquivo protegido que `open` aceita ler (a exportação passa de longe disto só com milhões de linhas).
const MAX_FILE_BYTES: usize = 256 * 1024 * 1024;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Custos do Argon2id. Ao LER um arquivo, valores fora destes limites são recusados: um arquivo hostil não pode pedir
/// gigabytes de memória nem minutos de processamento.
const MIN_M_KIB: u32 = 8;
const MAX_M_KIB: u32 = 262_144; // 256 MiB
const MAX_T: u32 = 10;
const MAX_P: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    /// O que o app usa de verdade: 64 MiB, 3 passadas, 1 faixa.
    pub const PRODUCTION: KdfParams = KdfParams { m_cost_kib: 65_536, t_cost: 3, p_cost: 1 };
    /// SÓ PARA TESTES: o mínimo possível, para não gastar segundos por teste. Nunca use fora deles.
    pub const FAST_FOR_TESTS: KdfParams = KdfParams { m_cost_kib: 8, t_cost: 1, p_cost: 1 };

    fn within_limits(self) -> bool {
        (MIN_M_KIB..=MAX_M_KIB).contains(&self.m_cost_kib) && (1..=MAX_T).contains(&self.t_cost) && (1..=MAX_P).contains(&self.p_cost)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectError {
    /// Menos de `MIN_PASSWORD_CHARS` caracteres (ou só espaços).
    PasswordTooShort,
    /// Mais de `MAX_PASSWORD_BYTES` bytes.
    PasswordTooLong,
    /// Parâmetros inválidos ou falha interna da biblioteca.
    Internal,
}

impl ProtectError {
    /// Código estável para a interface traduzir.
    pub fn code(self) -> &'static str {
        match self {
            ProtectError::PasswordTooShort => "export.password.short",
            ProtectError::PasswordTooLong => "export.password.long",
            ProtectError::Internal => "export.protect",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Não é um arquivo no formato protegido (outro formato, JSON quebrado, campos que faltam ou com tamanho errado).
    NotProtected,
    /// Formato reconhecido, mas com versão ou custos de derivação fora dos limites seguros.
    Unsupported,
    /// Senha errada OU arquivo alterado: de propósito, não se distingue um do outro.
    WrongPasswordOrCorrupt,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    kdf: String,
    #[serde(rename = "mCostKib")]
    m_cost_kib: u32,
    #[serde(rename = "tCost")]
    t_cost: u32,
    #[serde(rename = "pCost")]
    p_cost: u32,
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn check_password(password: &str) -> Result<(), ProtectError> {
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ProtectError::PasswordTooLong);
    }
    if password.trim().is_empty() || password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ProtectError::PasswordTooShort);
    }
    Ok(())
}

/// Valida a senha sem fazer mais nada (a interface e o motor usam para recusar antes de gerar a exportação).
pub fn validate_password(password: &str) -> Result<(), ProtectError> {
    check_password(password)
}

fn aad(p: KdfParams, salt: &[u8]) -> Vec<u8> {
    let mut a = format!("{FORMAT}|{KDF_NAME}|{}|{}|{}|", p.m_cost_kib, p.t_cost, p.p_cost).into_bytes();
    a.extend_from_slice(salt);
    a
}

fn derive(password: &str, salt: &[u8], p: KdfParams) -> Option<[u8; 32]> {
    let params = Params::new(p.m_cost_kib, p.t_cost, p.p_cost, Some(32)).ok()?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params).hash_password_into(password.as_bytes(), salt, &mut key).ok()?;
    Some(key)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Protege `plain` (os bytes do JSON da exportação) com a senha. Devolve o texto do arquivo protegido.
pub fn protect(plain: &[u8], password: &str, params: KdfParams) -> Result<String, ProtectError> {
    check_password(password)?;
    if !params.within_limits() {
        return Err(ProtectError::Internal);
    }
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let mut key = derive(password, &salt, params).ok_or(ProtectError::Internal)?;
    let cipher = Aes256Gcm::new((&key).into());
    key.fill(0);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher.encrypt(&nonce, Payload { msg: plain, aad: &aad(params, &salt) }).map_err(|_| ProtectError::Internal)?;
    let env = Envelope {
        format: FORMAT.into(),
        kdf: KDF_NAME.into(),
        m_cost_kib: params.m_cost_kib,
        t_cost: params.t_cost,
        p_cost: params.p_cost,
        salt: hex(&salt),
        nonce: hex(nonce.as_slice()),
        ciphertext: hex(&ct),
    };
    serde_json::to_string_pretty(&env).map_err(|_| ProtectError::Internal)
}

/// Lê um arquivo protegido e devolve os bytes do JSON da exportação. Trata o arquivo como hostil: confere formato, tamanhos
/// e limites de custo ANTES de gastar memória na derivação.
pub fn open(file: &[u8], password: &str) -> Result<Vec<u8>, OpenError> {
    if file.len() > MAX_FILE_BYTES {
        return Err(OpenError::Unsupported);
    }
    let env: Envelope = serde_json::from_slice(file).map_err(|_| OpenError::NotProtected)?;
    if env.format != FORMAT {
        return Err(OpenError::NotProtected);
    }
    let params = KdfParams { m_cost_kib: env.m_cost_kib, t_cost: env.t_cost, p_cost: env.p_cost };
    if env.kdf != KDF_NAME || !params.within_limits() {
        return Err(OpenError::Unsupported);
    }
    let salt = unhex(&env.salt).filter(|s| s.len() == SALT_LEN).ok_or(OpenError::NotProtected)?;
    let nonce = unhex(&env.nonce).filter(|n| n.len() == NONCE_LEN).ok_or(OpenError::NotProtected)?;
    let ct = unhex(&env.ciphertext).filter(|c| c.len() >= TAG_LEN).ok_or(OpenError::NotProtected)?;
    // Uma senha fora dos limites nunca abriria um arquivo feito por este app: falha do mesmo jeito que a senha errada.
    if check_password(password).is_err() {
        return Err(OpenError::WrongPasswordOrCorrupt);
    }
    let mut key = derive(password, &salt, params).ok_or(OpenError::Unsupported)?;
    let cipher = Aes256Gcm::new((&key).into());
    key.fill(0);
    cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: &aad(params, &salt) }).map_err(|_| OpenError::WrongPasswordOrCorrupt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams::FAST_FOR_TESTS;
    const PLAIN: &[u8] = br#"{"format":"developer-blackbox-export/2","incident":{"exeName":"synth-app.exe"}}"#;
    const PW: &str = "correct horse battery";

    fn env_of(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    fn with(text: &str, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut v = env_of(text);
        edit(&mut v);
        serde_json::to_vec(&v).unwrap()
    }

    #[test]
    fn a_protected_file_opens_with_the_same_password_and_gives_back_exactly_the_original() {
        let text = protect(PLAIN, PW, FAST).unwrap();
        assert_eq!(open(text.as_bytes(), PW).unwrap(), PLAIN);
    }

    #[test]
    fn the_file_does_not_contain_the_plain_text_or_the_password() {
        let text = protect(PLAIN, PW, FAST).unwrap();
        assert!(!text.contains("synth-app") && !text.contains("exeName") && !text.contains("incident"), "{text}");
        assert!(!text.contains(PW) && !text.contains("horse"), "the password never reaches the file");
        let v = env_of(&text);
        assert_eq!(v["format"], FORMAT);
        assert_eq!(v["kdf"], "argon2id");
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["ciphertext", "format", "kdf", "mCostKib", "nonce", "pCost", "salt", "tCost"], "only these fields, no free text");
    }

    #[test]
    fn a_wrong_password_fails_without_saying_why_and_so_does_a_changed_file() {
        let text = protect(PLAIN, PW, FAST).unwrap();
        assert_eq!(open(text.as_bytes(), "another password!"), Err(OpenError::WrongPasswordOrCorrupt));
        assert_eq!(open(text.as_bytes(), "short"), Err(OpenError::WrongPasswordOrCorrupt), "a password outside the limits is just wrong");
        // um byte do texto cifrado
        let flipped = with(&text, |v| {
            let ct = v["ciphertext"].as_str().unwrap().to_owned();
            let mut b: Vec<char> = ct.chars().collect();
            b[0] = if b[0] == '0' { '1' } else { '0' };
            v["ciphertext"] = b.into_iter().collect::<String>().into();
        });
        assert_eq!(open(&flipped, PW), Err(OpenError::WrongPasswordOrCorrupt));
        // o cabeçalho é autenticado: mexer no custo (ainda dentro dos limites), no sal ou no nonce também falha
        for (field, value) in [("tCost", serde_json::json!(2)), ("mCostKib", serde_json::json!(16))] {
            let edited = with(&text, |v| v[field] = value);
            assert_eq!(open(&edited, PW), Err(OpenError::WrongPasswordOrCorrupt), "{field}");
        }
        // mais faixas exigem mais memória (8 KiB por faixa): muda as duas coisas juntas
        let lanes = with(&text, |v| {
            v["pCost"] = serde_json::json!(2);
            v["mCostKib"] = serde_json::json!(16);
        });
        assert_eq!(open(&lanes, PW), Err(OpenError::WrongPasswordOrCorrupt), "pCost");
        for field in ["salt", "nonce"] {
            let edited = with(&text, |v| {
                let s = v[field].as_str().unwrap().to_owned();
                let mut b: Vec<char> = s.chars().collect();
                b[0] = if b[0] == '0' { '1' } else { '0' };
                v[field] = b.into_iter().collect::<String>().into();
            });
            assert_eq!(open(&edited, PW), Err(OpenError::WrongPasswordOrCorrupt), "{field}");
        }
    }

    #[test]
    fn two_protections_of_the_same_text_differ_because_salt_and_nonce_are_random() {
        let a = protect(PLAIN, PW, FAST).unwrap();
        let b = protect(PLAIN, PW, FAST).unwrap();
        let (a, b) = (env_of(&a), env_of(&b));
        assert_ne!(a["salt"], b["salt"]);
        assert_ne!(a["nonce"], b["nonce"]);
        assert_ne!(a["ciphertext"], b["ciphertext"]);
    }

    #[test]
    fn weak_passwords_are_refused_before_anything_is_derived() {
        for bad in ["", "        ", "1234567", "abc", "\t\t\t\t\t\t\t\t"] {
            assert_eq!(protect(PLAIN, bad, FAST), Err(ProtectError::PasswordTooShort), "{bad:?}");
            assert_eq!(validate_password(bad), Err(ProtectError::PasswordTooShort));
        }
        assert!(protect(PLAIN, "12345678", FAST).is_ok(), "eight characters is the minimum");
        // caracteres, não bytes: oito letras acentuadas valem
        assert!(validate_password("ãéíõúçàâ").is_ok());
        let long = "a".repeat(MAX_PASSWORD_BYTES + 1);
        assert_eq!(protect(PLAIN, &long, FAST), Err(ProtectError::PasswordTooLong));
        assert!(validate_password(&"a".repeat(MAX_PASSWORD_BYTES)).is_ok());
        assert_eq!(ProtectError::PasswordTooShort.code(), "export.password.short");
        assert_eq!(ProtectError::PasswordTooLong.code(), "export.password.long");
    }

    #[test]
    fn a_hostile_file_cannot_ask_for_huge_costs_or_other_formats() {
        let text = protect(PLAIN, PW, FAST).unwrap();
        for (field, value) in [
            ("mCostKib", serde_json::json!(4_000_000)),
            ("mCostKib", serde_json::json!(0)),
            ("tCost", serde_json::json!(1000)),
            ("tCost", serde_json::json!(0)),
            ("pCost", serde_json::json!(64)),
            ("pCost", serde_json::json!(0)),
            ("kdf", serde_json::json!("md5")),
        ] {
            let edited = with(&text, |v| v[field] = value.clone());
            assert_eq!(open(&edited, PW), Err(OpenError::Unsupported), "{field} = {value}");
        }
        let other = with(&text, |v| v["format"] = "developer-blackbox-export/2".into());
        assert_eq!(open(&other, PW), Err(OpenError::NotProtected));
        assert_eq!(open(b"{}", PW), Err(OpenError::NotProtected));
        assert_eq!(open(b"not json at all", PW), Err(OpenError::NotProtected));
        assert_eq!(open(b"", PW), Err(OpenError::NotProtected));
        // tamanhos errados dos campos
        for (field, value) in [("salt", "00"), ("nonce", "0011"), ("ciphertext", "00"), ("salt", "zz"), ("ciphertext", "abc")] {
            let edited = with(&text, |v| v[field] = value.into());
            assert_eq!(open(&edited, PW), Err(OpenError::NotProtected), "{field}");
        }
    }

    #[test]
    fn the_cost_limits_are_exactly_these_ones() {
        let p = |m, t, p| KdfParams { m_cost_kib: m, t_cost: t, p_cost: p };
        assert!(p(8, 1, 1).within_limits() && p(262_144, 10, 4).within_limits(), "both ends are allowed");
        for out in [p(7, 1, 1), p(262_145, 1, 1), p(8, 0, 1), p(8, 11, 1), p(8, 1, 0), p(8, 1, 5)] {
            assert!(!out.within_limits(), "{out:?}");
        }
    }

    #[test]
    fn the_protect_call_refuses_costs_outside_the_limits() {
        let weak = KdfParams { m_cost_kib: 1, t_cost: 1, p_cost: 1 };
        let huge = KdfParams { m_cost_kib: MAX_M_KIB + 1, t_cost: 1, p_cost: 1 };
        assert_eq!(protect(PLAIN, PW, weak), Err(ProtectError::Internal));
        assert_eq!(protect(PLAIN, PW, huge), Err(ProtectError::Internal));
        assert!(KdfParams::PRODUCTION.within_limits());
        assert_eq!(
            KdfParams::PRODUCTION,
            KdfParams { m_cost_kib: 65_536, t_cost: 3, p_cost: 1 },
            "the production costs are a decision, not an accident"
        );
    }

    #[test]
    fn production_costs_round_trip_and_are_written_in_the_header() {
        let text = protect(PLAIN, PW, KdfParams::PRODUCTION).unwrap();
        let v = env_of(&text);
        assert_eq!((v["mCostKib"].as_u64(), v["tCost"].as_u64(), v["pCost"].as_u64()), (Some(65_536), Some(3), Some(1)));
        assert_eq!(open(text.as_bytes(), PW).unwrap(), PLAIN);
    }

    #[test]
    fn the_key_derivation_is_deterministic_and_depends_on_every_input() {
        // Vetor conferido com a implementação de referência em C do Argon2 (a mesma chave de 256 bits saiu de lá). Se mudar sem
        // querer, arquivos já protegidos deixam de abrir.
        let salt = [7u8; SALT_LEN];
        let key = derive("regression-vector-password", &salt, FAST).unwrap();
        assert_eq!(hex(&key), "5235554583d168733a1afb8e1cbb4c78a0612c2d8151941871924f3a1127f198");
        assert_ne!(key, derive("regression-vector-passworD", &salt, FAST).unwrap());
        assert_ne!(key, derive("regression-vector-password", &[8u8; SALT_LEN], FAST).unwrap());
        assert_ne!(key, derive("regression-vector-password", &salt, KdfParams { m_cost_kib: 16, ..FAST }).unwrap());
    }

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        assert_eq!(unhex(&hex(&[0, 1, 0xab, 0xff])).unwrap(), [0, 1, 0xab, 0xff]);
        assert!(unhex("abc").is_none() && unhex("zz").is_none() && unhex("é1").is_none());
    }
}
