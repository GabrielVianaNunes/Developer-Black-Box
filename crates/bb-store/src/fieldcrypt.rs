//! Cifra de campos sensíveis do SQLite (AES-256-GCM).
//!
//! Formato: `enc1:` + hex(nonce[12] || texto cifrado + tag). O contexto (tabela.coluna) entra como
//! dado autenticado, então um valor cifrado não pode ser copiado para outro campo. A subchave é
//! derivada da chave mestra (protegida por DPAPI), separada da que cifra os segmentos.
//! Valores sem o prefixo são texto claro legado e continuam legíveis (para migração).

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use sha2::{Digest, Sha256};

pub const PREFIX: &str = "enc1:";

pub fn is_encrypted(s: &str) -> bool {
    s.starts_with(PREFIX)
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

pub struct FieldCipher(Aes256Gcm);

impl FieldCipher {
    pub fn new(master: &[u8; 32]) -> Self {
        let mut h = Sha256::new();
        h.update(b"developer-blackbox/store/v1");
        h.update(master);
        let sub = h.finalize();
        FieldCipher(Aes256Gcm::new_from_slice(&sub).expect("32-byte key"))
    }

    pub fn encrypt(&self, ctx: &str, plain: &str) -> String {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self
            .0
            .encrypt(&nonce, Payload { msg: plain.as_bytes(), aad: ctx.as_bytes() })
            .expect("AES-GCM encryption of a small value cannot fail");
        let mut raw = nonce.to_vec();
        raw.extend_from_slice(&ct);
        format!("{PREFIX}{}", hex(&raw))
    }

    /// `None` se estiver cifrado e não autenticar (chave errada ou valor adulterado/trocado).
    /// Texto sem o prefixo é devolvido como está (dado legado).
    pub fn decrypt(&self, ctx: &str, stored: &str) -> Option<String> {
        let Some(body) = stored.strip_prefix(PREFIX) else { return Some(stored.to_owned()) };
        let raw = unhex(body)?;
        if raw.len() < 12 + 16 {
            return None;
        }
        let (nonce, ct) = raw.split_at(12);
        let pt = self.0.decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: ctx.as_bytes() }).ok()?;
        String::from_utf8(pt).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_random_nonce() {
        let c = FieldCipher::new(&[1; 32]);
        let a = c.encrypt("t.c", "secret-value");
        let b = c.encrypt("t.c", "secret-value");
        assert_ne!(a, b, "a fresh nonce per value");
        assert!(is_encrypted(&a) && !a.contains("secret"));
        assert_eq!(c.decrypt("t.c", &a).as_deref(), Some("secret-value"));
    }

    #[test]
    fn the_context_is_authenticated() {
        let c = FieldCipher::new(&[1; 32]);
        let v = c.encrypt("notes.text", "x");
        assert_eq!(c.decrypt("incidents.exe_name", &v), None, "cannot be moved to another column");
    }

    #[test]
    fn a_different_key_or_tampering_fails() {
        let v = FieldCipher::new(&[1; 32]).encrypt("t.c", "x");
        assert_eq!(FieldCipher::new(&[2; 32]).decrypt("t.c", &v), None);
        let mut bad = v.clone();
        let last = bad.pop().unwrap();
        bad.push(if last == '0' { '1' } else { '0' });
        assert_eq!(FieldCipher::new(&[1; 32]).decrypt("t.c", &bad), None);
    }

    #[test]
    fn legacy_plaintext_passes_through() {
        assert_eq!(FieldCipher::new(&[1; 32]).decrypt("t.c", "plain").as_deref(), Some("plain"));
    }

    #[test]
    fn garbage_after_the_prefix_is_rejected() {
        let c = FieldCipher::new(&[1; 32]);
        for bad in ["enc1:", "enc1:zz", "enc1:abc", "enc1:00"] {
            assert_eq!(c.decrypt("t.c", bad), None, "{bad}");
        }
    }
}
