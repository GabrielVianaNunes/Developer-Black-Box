//! Formato do segmento selado: cabeçalho autenticado + zstd(NDJSON) cifrado com AES-256-GCM.
//!
//! ```text
//! magic "BBSEG001" (8) | index u64 (8) | session_id (16) | prev_hash (32)
//! | event_count u32 (4) | nonce (12)          -> cabeçalho de 80 bytes (é o AAD)
//! ciphertext (zstd do NDJSON) + tag GCM (16)
//! ```

use aes_gcm::aead::{Aead, AeadCore, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use sha2::{Digest, Sha256};

use crate::error::{RecorderError, Result};

pub const MAGIC: &[u8; 8] = b"BBSEG001";
pub const HEADER_LEN: usize = 80;
pub type Hash = [u8; 32];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentHeader {
    pub index: u64,
    pub session_id: [u8; 16],
    pub prev_hash: Hash,
    pub event_count: u32,
    pub nonce: [u8; 12],
}

impl SegmentHeader {
    fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..8].copy_from_slice(MAGIC);
        b[8..16].copy_from_slice(&self.index.to_le_bytes());
        b[16..32].copy_from_slice(&self.session_id);
        b[32..64].copy_from_slice(&self.prev_hash);
        b[64..68].copy_from_slice(&self.event_count.to_le_bytes());
        b[68..80].copy_from_slice(&self.nonce);
        b
    }

    fn decode(b: &[u8]) -> Result<Self> {
        if b.len() < HEADER_LEN || &b[0..8] != MAGIC {
            return Err(RecorderError::Corrupt("bad segment header"));
        }
        let mut h = SegmentHeader {
            index: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            session_id: [0; 16],
            prev_hash: [0; 32],
            event_count: u32::from_le_bytes(b[64..68].try_into().unwrap()),
            nonce: [0; 12],
        };
        h.session_id.copy_from_slice(&b[16..32]);
        h.prev_hash.copy_from_slice(&b[32..64]);
        h.nonce.copy_from_slice(&b[68..80]);
        Ok(h)
    }
}

pub fn hash(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

/// Comprime e cifra `lines` (uma por evento) num arquivo de segmento completo.
pub fn seal(
    cipher: &Aes256Gcm,
    index: u64,
    session_id: [u8; 16],
    prev_hash: Hash,
    lines: &[String],
) -> Result<Vec<u8>> {
    let compressed = zstd::encode_all(lines.join("\n").as_bytes(), 3)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let header = SegmentHeader {
        index,
        session_id,
        prev_hash,
        event_count: lines.len() as u32,
        nonce: nonce.into(),
    };
    let head = header.encode();
    let ct = cipher
        .encrypt(&nonce, Payload { msg: &compressed, aad: &head })
        .map_err(|_| RecorderError::Crypto)?;
    let mut out = Vec::with_capacity(HEADER_LEN + ct.len());
    out.extend_from_slice(&head);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Autentica, decifra e descomprime. Qualquer alteração de byte falha em `Crypto`.
pub fn open(cipher: &Aes256Gcm, bytes: &[u8]) -> Result<(SegmentHeader, Vec<String>)> {
    let header = SegmentHeader::decode(bytes)?;
    let (head, ct) = bytes.split_at(HEADER_LEN);
    let compressed = cipher
        .decrypt(Nonce::from_slice(&header.nonce), Payload { msg: ct, aad: head })
        .map_err(|_| RecorderError::Crypto)?;
    let plain = zstd::decode_all(&compressed[..])?;
    let text = String::from_utf8(plain).map_err(|_| RecorderError::Corrupt("non-utf8 payload"))?;
    let lines: Vec<String> = if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(str::to_owned).collect()
    };
    if lines.len() as u32 != header.event_count {
        return Err(RecorderError::Corrupt("event count mismatch"));
    }
    Ok((header, lines))
}

pub fn read_header(bytes: &[u8]) -> Result<SegmentHeader> {
    SegmentHeader::decode(bytes)
}
