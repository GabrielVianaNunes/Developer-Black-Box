//! Verificação de um instalador baixado: SHA-256 calculado aqui e assinatura Ed25519.
//!
//! O app só aceita uma atualização cuja assinatura confere com uma das chaves públicas embutidas abaixo.
//! A assinatura cobre a VERSÃO e o SHA-256 do arquivo (veja `scripts/sign-release.mjs`), então:
//!   * um arquivo adulterado ou corrompido não confere;
//!   * um instalador antigo e legítimo não pode ser apresentado como uma versão mais nova.
//!
//! Nada aqui acessa rede ou disco além de ler o arquivo que já foi baixado.

use std::io::Read;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::Version;

/// Prefixo do texto assinado. Mude junto com `MESSAGE_PREFIX` de `scripts/sign-release.mjs` (há teste).
pub const MESSAGE_PREFIX: &str = "DeveloperBlackBox-release-v1";

/// Chaves públicas (hex) cujas assinaturas o app aceita. Mais de uma permite trocar a chave sem deixar
/// os apps já instalados sem poder atualizar: publica-se com a nova e mantém-se a antiga por um tempo.
pub const TRUSTED_PUBLIC_KEYS: [&str; 1] = ["9c53e2d440876b09c9ac1d5c115ad8fc2fa6af1591019411c61acd2b79b07884"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// O arquivo `.sig` não tem o formato esperado (128 caracteres hexadecimais).
    BadSignatureFormat,
    /// A assinatura não confere com nenhuma chave de confiança para este arquivo e esta versão.
    Mismatch,
    /// Não foi possível ler o arquivo.
    Unreadable,
}

impl VerifyError {
    pub fn code(&self) -> &'static str {
        match self {
            VerifyError::BadSignatureFormat => "update.bad_signature",
            VerifyError::Mismatch => "update.signature_mismatch",
            VerifyError::Unreadable => "update.unreadable",
        }
    }
}

fn hex_decode(s: &str, len: usize) -> Option<Vec<u8>> {
    if s.len() != len * 2 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    (0..len).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (hex minúsculo) de tudo o que `reader` devolver.
pub fn sha256_hex(mut reader: impl Read) -> Result<String, VerifyError> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|_| VerifyError::Unreadable)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// O texto exato que é assinado.
pub fn signed_message(version: &Version, sha256_hex: &str) -> Vec<u8> {
    format!("{MESSAGE_PREFIX}\n{version}\n{sha256_hex}\n").into_bytes()
}

/// Confere a assinatura (`signature_hex`, o conteúdo do `.sig`) contra as chaves `trusted_hex`.
/// `verify_strict` recusa assinaturas maleáveis e chaves de ordem pequena.
pub fn verify_release(trusted_hex: &[&str], version: &Version, sha256_hex: &str, signature_hex: &str) -> Result<(), VerifyError> {
    let sig_bytes = hex_decode(signature_hex.trim_end_matches(['\r', '\n']), 64).ok_or(VerifyError::BadSignatureFormat)?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|_| VerifyError::BadSignatureFormat)?;
    let message = signed_message(version, sha256_hex);
    let ok = trusted_hex.iter().any(|k| {
        hex_decode(k, 32)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .and_then(|b| VerifyingKey::from_bytes(&b).ok())
            .is_some_and(|key| key.verify_strict(&message, &signature).is_ok())
    });
    if ok { Ok(()) } else { Err(VerifyError::Mismatch) }
}

/// Chaves em que o app confia: as embutidas acima. Num build de TESTE (feature `e2e`) aceita também a chave
/// pública em `BB_E2E_TRUSTED_KEY`; em builds normais essa variável é ignorada e nem existe no código.
pub fn trusted_keys() -> Vec<String> {
    #[allow(unused_mut)]
    let mut keys: Vec<String> = TRUSTED_PUBLIC_KEYS.iter().map(|k| (*k).to_owned()).collect();
    #[cfg(feature = "e2e")]
    if let Ok(extra) = std::env::var("BB_E2E_TRUSTED_KEY") {
        keys.push(extra);
    }
    keys
}

/// Verificação completa de um instalador já baixado, com as chaves embutidas no app.
pub fn verify_installer(version: &Version, installer: impl Read, signature_hex: &str) -> Result<(), VerifyError> {
    let sha = sha256_hex(installer)?;
    let keys = trusted_keys();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    verify_release(&keys, version, &sha, signature_hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// Par de chaves SÓ para teste (semente fixa e pública; não tem relação com a chave real de release).
    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn pub_hex(k: &SigningKey) -> String {
        hex_encode(k.verifying_key().as_bytes())
    }

    fn sign(k: &SigningKey, version: &str, file: &[u8]) -> (String, String) {
        let sha = sha256_hex(file).unwrap();
        let sig = k.sign(&signed_message(&v(version), &sha));
        (sha, hex_encode(&sig.to_bytes()))
    }

    #[test]
    fn sha256_matches_the_known_vectors() {
        assert_eq!(sha256_hex(&b""[..]).unwrap(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(&b"abc"[..]).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_valid_signature_is_accepted() {
        let k = test_key();
        let (sha, sig) = sign(&k, "0.2.0", b"installer bytes");
        assert_eq!(verify_release(&[&pub_hex(&k)], &v("0.2.0"), &sha, &sig), Ok(()));
        assert_eq!(verify_release(&[&pub_hex(&k)], &v("0.2.0"), &sha, &format!("{sig}\n")), Ok(()), "trailing newline is fine");
    }

    #[test]
    fn a_tampered_file_is_rejected() {
        let k = test_key();
        let (_, sig) = sign(&k, "0.2.0", b"installer bytes");
        let other = sha256_hex(&b"installer bytez"[..]).unwrap();
        assert_eq!(verify_release(&[&pub_hex(&k)], &v("0.2.0"), &other, &sig), Err(VerifyError::Mismatch));
    }

    #[test]
    fn an_old_signed_installer_cannot_pose_as_a_newer_version() {
        let k = test_key();
        let (sha, sig) = sign(&k, "0.1.0", b"old installer");
        assert_eq!(verify_release(&[&pub_hex(&k)], &v("0.1.0"), &sha, &sig), Ok(()));
        assert_eq!(verify_release(&[&pub_hex(&k)], &v("9.9.9"), &sha, &sig), Err(VerifyError::Mismatch));
    }

    #[test]
    fn a_signature_from_an_untrusted_key_is_rejected() {
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let (sha, sig) = sign(&attacker, "0.2.0", b"malware");
        assert_eq!(verify_release(&[&pub_hex(&test_key())], &v("0.2.0"), &sha, &sig), Err(VerifyError::Mismatch));
        assert_eq!(verify_release(&[], &v("0.2.0"), &sha, &sig), Err(VerifyError::Mismatch), "no trusted key means nothing is accepted");
    }

    #[test]
    fn any_of_several_trusted_keys_is_enough_for_key_rotation() {
        let (old, new) = (test_key(), SigningKey::from_bytes(&[8u8; 32]));
        let (sha, sig) = sign(&new, "0.3.0", b"installer");
        assert_eq!(verify_release(&[&pub_hex(&old), &pub_hex(&new)], &v("0.3.0"), &sha, &sig), Ok(()));
    }

    #[test]
    fn malformed_signature_files_are_rejected_before_any_crypto() {
        let k = test_key();
        let (sha, good) = sign(&k, "0.2.0", b"x");
        let keys = [pub_hex(&k)];
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        for bad in ["", "zz", &good[..127], &format!("{good}00"), &good.to_uppercase(), &format!(" {good}"), &format!("{good} extra"), "not hex at all"] {
            assert_eq!(verify_release(&keys, &v("0.2.0"), &sha, bad), Err(VerifyError::BadSignatureFormat), "{bad:?}");
        }
        // Um bit alterado continua sendo 128 hex válidos, mas não confere.
        let mut flipped = good.into_bytes();
        flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
        assert_eq!(verify_release(&keys, &v("0.2.0"), &sha, std::str::from_utf8(&flipped).unwrap()), Err(VerifyError::Mismatch));
    }

    #[test]
    fn the_embedded_trusted_keys_are_well_formed() {
        assert!(!TRUSTED_PUBLIC_KEYS.is_empty());
        for k in TRUSTED_PUBLIC_KEYS {
            let b = <[u8; 32]>::try_from(hex_decode(k, 32).expect("64 lowercase hex chars")).unwrap();
            VerifyingKey::from_bytes(&b).expect("a valid Ed25519 public key");
        }
    }

    #[test]
    fn the_real_installer_path_rejects_a_signature_made_with_another_key() {
        let (_, sig) = sign(&test_key(), "0.2.0", b"installer");
        assert_eq!(verify_installer(&v("0.2.0"), &b"installer"[..], &sig), Err(VerifyError::Mismatch));
    }

    #[test]
    fn error_codes_are_neutral_and_distinct() {
        let all = [VerifyError::BadSignatureFormat, VerifyError::Mismatch, VerifyError::Unreadable];
        let codes: std::collections::BTreeSet<_> = all.iter().map(VerifyError::code).collect();
        assert_eq!(codes.len(), all.len());
    }
}

#[cfg(test)]
mod cross_language {
    use super::*;

    /// Vetor gerado pelo `scripts/sign-release.mjs` (Node): o Rust tem de aceitá-lo e recusar qualquer mudança.
    /// Prova que os dois lados montam o mesmo texto assinado e usam a mesma codificação.
    const VECTOR: &str = include_str!("../../../tests/privacy/release_signature_vector.json");

    fn field(name: &str) -> String {
        serde_json::from_str::<serde_json::Value>(VECTOR).unwrap()[name].as_str().unwrap().to_owned()
    }

    #[test]
    fn a_signature_made_by_the_node_script_verifies_in_rust() {
        let (key, version, file, sig) = (field("publicKey"), Version::parse(&field("version")).unwrap(), field("fileUtf8"), field("signature"));
        let sha = sha256_hex(file.as_bytes()).unwrap();
        assert_eq!(verify_release(&[&key], &version, &sha, &sig), Ok(()));
        // Qualquer mudança (arquivo, versão ou chave) é recusada.
        let other_sha = sha256_hex(format!("{file}!").as_bytes()).unwrap();
        assert_eq!(verify_release(&[&key], &version, &other_sha, &sig), Err(VerifyError::Mismatch));
        assert_eq!(verify_release(&[&key], &Version::parse("0.2.1").unwrap(), &sha, &sig), Err(VerifyError::Mismatch));
        assert_eq!(verify_release(&[TRUSTED_PUBLIC_KEYS[0]], &version, &sha, &sig), Err(VerifyError::Mismatch));
    }

    #[test]
    fn the_signed_text_prefix_is_the_same_in_node_and_rust() {
        let js = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/sign-release.mjs")).unwrap();
        assert!(js.contains(&format!("MESSAGE_PREFIX = \"{MESSAGE_PREFIX}\"")), "prefix differs from scripts/sign-release.mjs");
    }
}
