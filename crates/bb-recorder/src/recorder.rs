//! Recorder: journal cifrado por evento + segmentos selados em cadeia.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aes_gcm::aead::{rand_core::RngCore, Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use bb_core::ValidatedEvent;

use crate::error::{RecorderError, Result};
use crate::keys::KeyProvider;
use crate::segment::{self, Hash};

const JOURNAL_NAME: &str = "journal.bbwal";
const JOURNAL_MAGIC: &[u8; 8] = b"BBWAL001";
const JOURNAL_HEADER_LEN: usize = 24;
const NONCE_LEN: usize = 12;
const PRUNED_LOG: &str = "pruned.log";

#[derive(Clone, Debug)]
pub struct RecorderConfig {
    pub max_events_per_segment: u32,
    pub max_journal_bytes: u64,
    pub max_total_bytes: u64,
    /// Segmentos mais antigos que isto (mtime) são removidos, exceto os preservados.
    pub max_age: Option<Duration>,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            max_events_per_segment: 5_000,
            max_journal_bytes: 1 << 20,
            max_total_bytes: 256 << 20,
            max_age: Some(Duration::from_secs(24 * 3600)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentInfo {
    pub index: u64,
    pub size: u64,
    pub preserved: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub segments: usize,
    pub events: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RecoveryReport {
    pub recovered_events: usize,
    /// O final do journal estava truncado ou corrompido e foi descartado.
    pub discarded_tail: bool,
}

struct Journal {
    file: File,
    frames: u64,
    bytes: u64,
    header: [u8; JOURNAL_HEADER_LEN],
}

pub struct Recorder {
    dir: PathBuf,
    cipher: Aes256Gcm,
    cfg: RecorderConfig,
    session_id: [u8; 16],
    journal: Option<Journal>,
    next_index: u64,
    last_hash: Hash,
    sealed_bytes: u64,
    recovery: Option<RecoveryReport>,
}

fn segment_path(dir: &Path, index: u64) -> PathBuf {
    dir.join(format!("seg-{index:010}.bbseg"))
}
fn keep_path(dir: &Path, index: u64) -> PathBuf {
    dir.join(format!("seg-{index:010}.keep"))
}

fn parse_index(name: &str) -> Option<u64> {
    name.strip_prefix("seg-")?.strip_suffix(".bbseg")?.parse().ok()
}

fn frame_aad(header: &[u8; JOURNAL_HEADER_LEN], n: u64) -> Vec<u8> {
    let mut a = header.to_vec();
    a.extend_from_slice(&n.to_le_bytes());
    a
}

impl Recorder {
    /// Abre o diretório de dados, recuperando um journal deixado por uma queda.
    pub fn open(dir: impl AsRef<Path>, keys: &dyn KeyProvider, cfg: RecorderConfig) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let cipher = Aes256Gcm::new_from_slice(&keys.key()?).map_err(|_| RecorderError::Crypto)?;
        let mut session_id = [0u8; 16];
        OsRng.fill_bytes(&mut session_id);
        let mut r = Recorder {
            dir,
            cipher,
            cfg,
            session_id,
            journal: None,
            next_index: 0,
            last_hash: [0; 32],
            sealed_bytes: 0,
            recovery: None,
        };
        r.scan_sealed()?;
        r.recover_journal()?;
        Ok(r)
    }

    pub fn config(&self) -> &RecorderConfig {
        &self.cfg
    }

    /// Aplica novos limites de retenção e de armazenamento. A retenção é reavaliada na hora.
    pub fn set_config(&mut self, cfg: RecorderConfig) -> Result<Vec<u64>> {
        self.cfg = cfg;
        self.enforce_retention()
    }

    /// Bytes usados em disco: segmentos selados + journal ativo.
    pub fn storage_bytes(&self) -> u64 {
        self.sealed_bytes + self.journal.as_ref().map_or(0, |j| j.bytes)
    }

    /// Eventos do journal ativo ainda não selados (autenticados e decifrados).
    /// Vazio se não houver journal ou se ele estiver ilegível.
    pub fn journal_lines(&self) -> Vec<String> {
        let path = self.dir.join(JOURNAL_NAME);
        if !path.exists() {
            return Vec::new();
        }
        self.read_journal(&path).map(|(_, lines, _)| lines).unwrap_or_default()
    }

    /// Relatório da recuperação feita em `open`, se havia journal órfão.
    pub fn recovery_report(&self) -> Option<&RecoveryReport> {
        self.recovery.as_ref()
    }

    /// Único caminho de escrita: só aceita eventos que passaram pelo Guard.
    pub fn append(&mut self, event: &ValidatedEvent) -> Result<()> {
        let line = serde_json::to_string(event).map_err(|_| RecorderError::Corrupt("serialize"))?;
        self.ensure_room(line.len() as u64 + 64)?;
        let (header, n) = {
            let j = self.journal_mut()?;
            (j.header, j.frames)
        };
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let aad = frame_aad(&header, n);
        let ct = self
            .cipher
            .encrypt(&nonce, Payload { msg: line.as_bytes(), aad: &aad })
            .map_err(|_| RecorderError::Crypto)?;
        let mut frame = Vec::with_capacity(4 + NONCE_LEN + ct.len());
        frame.extend_from_slice(&(ct.len() as u32).to_le_bytes());
        frame.extend_from_slice(&nonce);
        frame.extend_from_slice(&ct);
        let (max_events, max_bytes) = (u64::from(self.cfg.max_events_per_segment), self.cfg.max_journal_bytes);
        let j = self.journal_mut()?;
        j.file.write_all(&frame)?;
        j.frames += 1;
        j.bytes += frame.len() as u64;
        let full = j.frames >= max_events || j.bytes >= max_bytes;
        if full {
            // O evento já está persistido no journal; se a rotação falhar
            // (ex. disco cheio), tenta de novo na próxima chamada.
            let _ = self.seal();
        }
        Ok(())
    }

    /// Sela o journal atual num segmento cifrado, comprimido e encadeado.
    pub fn seal(&mut self) -> Result<Option<u64>> {
        let path = self.dir.join(JOURNAL_NAME);
        if self.journal.is_none() && !path.exists() {
            return Ok(None);
        }
        if let Some(j) = &self.journal {
            j.file.sync_data()?;
        }
        let (_, lines, _) = self.read_journal(&path)?;
        let sealed = self.write_segment(&lines)?;
        self.journal = None;
        fs::remove_file(&path)?;
        if sealed.is_some() {
            self.enforce_retention()?;
        }
        Ok(sealed)
    }

    pub fn list_segments(&self) -> Result<Vec<SegmentInfo>> {
        let mut v = Vec::new();
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            let name = e.file_name();
            if let Some(index) = name.to_str().and_then(parse_index) {
                if e.file_type()?.is_file() {
                    v.push(SegmentInfo {
                        index,
                        size: e.metadata()?.len(),
                        preserved: keep_path(&self.dir, index).exists(),
                    });
                }
            }
        }
        v.sort_by_key(|s| s.index);
        Ok(v)
    }

    pub fn read_segment(&self, index: u64) -> Result<Vec<String>> {
        let bytes = fs::read(segment_path(&self.dir, index))?;
        Ok(segment::open(&self.cipher, &bytes)?.1)
    }

    /// Menor e maior `ts_utc_ms` dos eventos do segmento.
    pub fn time_range(&self, index: u64) -> Result<Option<(i64, i64)>> {
        let mut range: Option<(i64, i64)> = None;
        for line in self.read_segment(index)? {
            let v: serde_json::Value =
                serde_json::from_str(&line).map_err(|_| RecorderError::Corrupt("bad event json"))?;
            if let Some(ts) = v.get("ts_utc_ms").and_then(|t| t.as_i64()) {
                range = Some(match range {
                    Some((lo, hi)) => (lo.min(ts), hi.max(ts)),
                    None => (ts, ts),
                });
            }
        }
        Ok(range)
    }

    /// Índices dos segmentos com algum evento em `[from_utc_ms, to_utc_ms]`.
    pub fn segments_overlapping(&self, from_utc_ms: i64, to_utc_ms: i64) -> Result<Vec<u64>> {
        let mut out = Vec::new();
        for s in self.list_segments()? {
            if let Some((lo, hi)) = self.time_range(s.index)? {
                if hi >= from_utc_ms && lo <= to_utc_ms {
                    out.push(s.index);
                }
            }
        }
        Ok(out)
    }

    /// Marca o segmento como evidência: retenção não o remove.
    pub fn preserve(&self, index: u64) -> Result<()> {
        if !segment_path(&self.dir, index).is_file() {
            return Err(RecorderError::Corrupt("no such segment"));
        }
        File::create(keep_path(&self.dir, index))?;
        Ok(())
    }

    pub fn unpreserve(&self, index: u64) -> Result<()> {
        let p = keep_path(&self.dir, index);
        if p.exists() {
            fs::remove_file(p)?;
        }
        Ok(())
    }

    /// Verifica autenticidade de cada segmento e a cadeia de hashes.
    /// Lacunas só são aceitas para segmentos removidos pela retenção.
    pub fn verify(&self) -> Result<VerifyReport> {
        let pruned = self.read_pruned()?;
        let mut prev: Option<(u64, Hash)> = None;
        let mut events = 0u64;
        let segs = self.list_segments()?;
        for s in &segs {
            let bytes = fs::read(segment_path(&self.dir, s.index))?;
            let (h, lines) = segment::open(&self.cipher, &bytes)?;
            if h.index != s.index {
                return Err(RecorderError::Corrupt("index does not match file name"));
            }
            if let Some((pi, ph)) = prev {
                if s.index == pi + 1 {
                    if h.prev_hash != ph {
                        return Err(RecorderError::Corrupt("hash chain broken"));
                    }
                } else if !((pi + 1)..s.index).all(|i| pruned.contains(&i)) {
                    return Err(RecorderError::Corrupt("missing segment"));
                }
            }
            events += lines.len() as u64;
            prev = Some((s.index, segment::hash(&bytes)));
        }
        Ok(VerifyReport { segments: segs.len(), events })
    }

    /// Remove segmentos por idade e por cota, do mais antigo ao mais novo,
    /// nunca os preservados. Devolve os índices removidos.
    pub fn enforce_retention(&mut self) -> Result<Vec<u64>> {
        self.prune(0)
    }

    /// Como `enforce_retention`, mas também libera `reserve` bytes de folga.
    fn prune(&mut self, reserve: u64) -> Result<Vec<u64>> {
        let mut removed = Vec::new();
        let now = SystemTime::now();
        let mut segs = self.list_segments()?;
        let mut total: u64 = segs.iter().map(|s| s.size).sum();
        // O segmento mais novo nunca é removido: se a cota estiver cheia de
        // evidências preservadas, o recorder recusa novos eventos (QuotaExceeded)
        // em vez de descartar em silêncio o que acabou de gravar.
        let newest = segs.last().map(|s| s.index);
        segs.retain(|s| !s.preserved && Some(s.index) != newest);
        for s in segs {
            let too_old = match self.cfg.max_age {
                Some(max) => fs::metadata(segment_path(&self.dir, s.index))
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| now.duration_since(t).ok())
                    .is_some_and(|age| age > max),
                None => false,
            };
            if too_old || total + reserve > self.cfg.max_total_bytes {
                self.record_pruned(s.index)?;
                fs::remove_file(segment_path(&self.dir, s.index))?;
                total = total.saturating_sub(s.size);
                removed.push(s.index);
            }
        }
        self.sealed_bytes = total;
        Ok(removed)
    }

    /// Exclui segmentos gravados (o journal ativo é selado antes). Evidências preservadas só
    /// saem com `include_preserved`. Devolve quantos segmentos foram removidos.
    pub fn delete_segments(&mut self, include_preserved: bool) -> Result<usize> {
        self.seal()?;
        let mut removed = 0;
        for s in self.list_segments()? {
            if s.preserved && !include_preserved {
                continue;
            }
            self.record_pruned(s.index)?;
            fs::remove_file(segment_path(&self.dir, s.index))?;
            self.unpreserve(s.index)?;
            removed += 1;
        }
        self.sealed_bytes = self.list_segments()?.iter().map(|s| s.size).sum();
        Ok(removed)
    }

    // ---- internos ----

    fn ensure_room(&mut self, incoming: u64) -> Result<()> {
        let used = |r: &Recorder| r.sealed_bytes + r.journal.as_ref().map_or(0, |j| j.bytes);
        if used(self) + incoming <= self.cfg.max_total_bytes {
            return Ok(());
        }
        self.seal()?;
        self.prune(incoming + self.journal.as_ref().map_or(0, |j| j.bytes))?;
        if used(self) + incoming <= self.cfg.max_total_bytes {
            Ok(())
        } else {
            Err(RecorderError::QuotaExceeded)
        }
    }

    fn journal_mut(&mut self) -> Result<&mut Journal> {
        if self.journal.is_none() {
            let mut header = [0u8; JOURNAL_HEADER_LEN];
            header[0..8].copy_from_slice(JOURNAL_MAGIC);
            header[8..24].copy_from_slice(&self.session_id);
            let mut file = OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(self.dir.join(JOURNAL_NAME))?;
            file.write_all(&header)?;
            self.journal = Some(Journal { file, frames: 0, bytes: JOURNAL_HEADER_LEN as u64, header });
        }
        Ok(self.journal.as_mut().expect("just created"))
    }

    fn scan_sealed(&mut self) -> Result<()> {
        let segs = self.list_segments()?;
        self.sealed_bytes = segs.iter().map(|s| s.size).sum();
        if let Some(last) = segs.last() {
            let bytes = fs::read(segment_path(&self.dir, last.index))?;
            self.last_hash = segment::hash(&bytes);
            self.next_index = last.index + 1;
        }
        // Índices já podados nunca são reutilizados.
        if let Some(max) = self.read_pruned()?.into_iter().max() {
            self.next_index = self.next_index.max(max + 1);
        }
        Ok(())
    }

    fn recover_journal(&mut self) -> Result<()> {
        let path = self.dir.join(JOURNAL_NAME);
        if !path.exists() {
            return Ok(());
        }
        let (session, lines, tail) = match self.read_journal(&path) {
            Ok(v) => v,
            Err(_) => {
                // Cabeçalho ilegível: mantém o arquivo para inspeção e segue.
                fs::rename(&path, self.dir.join("journal.bbwal.corrupt"))?;
                self.recovery = Some(RecoveryReport { recovered_events: 0, discarded_tail: true });
                return Ok(());
            }
        };
        let recovered = lines.len();
        // Primeiro grava o segmento; só depois mexe no journal original.
        let prev_session = std::mem::replace(&mut self.session_id, session);
        let sealed = self.write_segment(&lines);
        self.session_id = prev_session;
        sealed?;
        let mut discarded = false;
        match tail {
            Tail::Complete => fs::remove_file(&path)?,
            Tail::Truncated => {
                discarded = true;
                fs::remove_file(&path)?;
            }
            Tail::Corrupt => {
                discarded = true;
                fs::rename(&path, self.dir.join("journal.bbwal.corrupt"))?;
            }
        }
        self.recovery = Some(RecoveryReport { recovered_events: recovered, discarded_tail: discarded });
        Ok(())
    }

    /// Lê e decifra os quadros válidos do journal. Nunca falha por causa da
    /// cauda: devolve o que conseguiu autenticar e como o arquivo terminou.
    fn read_journal(&self, path: &Path) -> Result<([u8; 16], Vec<String>, Tail)> {
        let mut data = Vec::new();
        File::open(path)?.read_to_end(&mut data)?;
        if data.len() < JOURNAL_HEADER_LEN || &data[0..8] != JOURNAL_MAGIC {
            return Err(RecorderError::Corrupt("bad journal header"));
        }
        let mut header = [0u8; JOURNAL_HEADER_LEN];
        header.copy_from_slice(&data[..JOURNAL_HEADER_LEN]);
        let mut session = [0u8; 16];
        session.copy_from_slice(&header[8..24]);
        let (mut pos, mut n, mut lines) = (JOURNAL_HEADER_LEN, 0u64, Vec::new());
        let tail = loop {
            if pos == data.len() {
                break Tail::Complete;
            }
            if data.len() - pos < 4 + NONCE_LEN {
                break Tail::Truncated;
            }
            let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            let end = pos + 4 + NONCE_LEN + len;
            if end > data.len() {
                break Tail::Truncated;
            }
            let nonce = Nonce::from_slice(&data[pos + 4..pos + 4 + NONCE_LEN]);
            let aad = frame_aad(&header, n);
            match self.cipher.decrypt(nonce, Payload { msg: &data[pos + 4 + NONCE_LEN..end], aad: &aad }) {
                Ok(pt) => match String::from_utf8(pt) {
                    Ok(s) => lines.push(s),
                    Err(_) => break Tail::Corrupt,
                },
                Err(_) if end == data.len() => break Tail::Truncated,
                Err(_) => break Tail::Corrupt,
            }
            pos = end;
            n += 1;
        };
        Ok((session, lines, tail))
    }

    /// Grava um segmento de forma atômica (tmp → fsync → rename). Se falhar,
    /// nada muda: segmentos anteriores e o journal continuam intactos.
    fn write_segment(&mut self, lines: &[String]) -> Result<Option<u64>> {
        if lines.is_empty() {
            return Ok(None);
        }
        let index = self.next_index;
        let bytes = segment::seal(&self.cipher, index, self.session_id, self.last_hash, lines)?;
        let final_path = segment_path(&self.dir, index);
        let tmp = final_path.with_extension("bbseg.tmp");
        let result = (|| -> Result<()> {
            let mut f = File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            fs::rename(&tmp, &final_path)?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        self.last_hash = segment::hash(&bytes);
        self.next_index = index + 1;
        self.sealed_bytes += bytes.len() as u64;
        Ok(Some(index))
    }

    fn record_pruned(&self, index: u64) -> Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(self.dir.join(PRUNED_LOG))?;
        writeln!(f, "{index}")?;
        f.sync_data()?;
        Ok(())
    }

    fn read_pruned(&self) -> Result<HashSet<u64>> {
        match fs::read_to_string(self.dir.join(PRUNED_LOG)) {
            Ok(s) => Ok(s.lines().filter_map(|l| l.trim().parse().ok()).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
            Err(e) => Err(e.into()),
        }
    }
}

enum Tail {
    Complete,
    Truncated,
    Corrupt,
}
