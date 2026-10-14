//! Limites de espaço dos dados do app: nada do que o app grava pode crescer sem teto.
//!
//! O que já tinha teto: os segmentos gravados (cota e idade da retenção). O que NÃO tinha, e passa a ter aqui:
//! - a pasta `exports` (cada exportação ficava para sempre);
//! - a evidência de incidentes (segmentos "preservados" escapam da retenção; sem teto, enchiam a cota e a gravação parava).
//!
//! As decisões são funções puras (recebem listas e devolvem o que remover), para serem testadas sem disco real.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bb_store::{CaptureState, IncidentKind, InvestigationState};

/// Quantas exportações ficam na pasta `exports` (as mais recentes).
pub const MAX_EXPORT_FILES: usize = 30;
/// Tamanho total máximo da pasta `exports`.
pub const MAX_EXPORT_BYTES: u64 = 100 << 20;
/// Parte da cota de armazenamento que a evidência de incidentes pode ocupar (o resto fica para a gravação contínua).
pub const EVIDENCE_SHARE_PCT: u64 = 50;
/// Linhas mantidas no histórico de configuração.
pub const MAX_CONFIG_HISTORY: usize = 1_000;

/// É um arquivo de exportação do app? (só estes são contados e removidos; nada mais na pasta é tocado)
pub fn is_export_file(name: &str) -> bool {
    name.starts_with("incident-") && name.ends_with(".json")
}

/// Quais arquivos remover para a pasta respeitar os dois limites. `files` = (nome, tamanho, quando foi modificado);
/// `keep` (o que acabou de ser escrito) nunca entra na lista. Do mais antigo ao mais novo; empate pelo nome.
pub fn exports_to_remove(files: &[(String, u64, SystemTime)], keep: &str, max_files: usize, max_bytes: u64) -> Vec<String> {
    let mut mine: Vec<&(String, u64, SystemTime)> = files.iter().filter(|f| is_export_file(&f.0)).collect();
    // Mais novo primeiro: o que sobrar do corte é o mais antigo.
    mine.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.0.cmp(&a.0)));
    let (mut count, mut bytes) = (0usize, 0u64);
    let mut out = Vec::new();
    for f in mine {
        let over = count + 1 > max_files || bytes + f.1 > max_bytes;
        if over && f.0 != keep {
            out.push(f.0.clone());
        } else {
            count += 1;
            bytes += f.1;
        }
    }
    out
}

/// Aplica `exports_to_remove` numa pasta de verdade. Devolve quantos arquivos removeu. Falhas de leitura/remoção não
/// quebram a exportação que acabou de dar certo: o que não foi possível apagar fica para a próxima.
pub fn prune_exports(dir: &Path, keep: &Path, max_files: usize, max_bytes: u64) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut files = Vec::new();
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        files.push((e.file_name().to_string_lossy().into_owned(), meta.len(), modified));
    }
    let keep_name = keep.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut removed = 0;
    for name in exports_to_remove(&files, &keep_name, max_files, max_bytes) {
        let path: PathBuf = dir.join(&name);
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// O que importa de um incidente para decidir se pode ser removido sozinho.
#[derive(Clone, Debug)]
pub struct EvidenceItem {
    pub id: i64,
    pub created_utc_ms: i64,
    pub kind: IncidentKind,
    pub state: InvestigationState,
    pub capture: CaptureState,
    pub has_notes: bool,
    pub segments: Vec<u64>,
}

impl EvidenceItem {
    /// Só vai embora sozinho: incidente AUTOMÁTICO, já com a captura concluída, que a pessoa ainda não tocou (novo ou
    /// descartado) e sem nenhuma anotação. Manual, em investigação, resolvido ou com anotações: nunca.
    pub fn removable(&self) -> bool {
        self.kind != IncidentKind::Manual
            && self.capture == CaptureState::Preserved
            && matches!(self.state, InvestigationState::New | InvestigationState::Dismissed)
            && !self.has_notes
    }
}

/// Bytes ocupados pela evidência: a união dos segmentos (um segmento compartilhado por vários incidentes conta uma vez).
pub fn evidence_bytes(items: &[&EvidenceItem], seg_sizes: &HashMap<u64, u64>) -> u64 {
    let segs: HashSet<u64> = items.iter().flat_map(|i| i.segments.iter().copied()).collect();
    segs.iter().map(|s| seg_sizes.get(s).copied().unwrap_or(0)).sum()
}

/// Quais incidentes remover para a evidência caber em `budget` bytes: do mais antigo ao mais novo, só os `removable`.
/// Para quando couber, ou quando não houver mais nenhum removível (a evidência da pessoa nunca é sacrificada).
pub fn evidence_to_remove(items: &[EvidenceItem], seg_sizes: &HashMap<u64, u64>, budget: u64) -> Vec<i64> {
    let mut remaining: Vec<&EvidenceItem> = items.iter().collect();
    let mut candidates: Vec<&EvidenceItem> = items.iter().filter(|i| i.removable()).collect();
    candidates.sort_by_key(|i| (i.created_utc_ms, i.id));
    let mut out = Vec::new();
    for c in candidates {
        if evidence_bytes(&remaining, seg_sizes) <= budget {
            break;
        }
        remaining.retain(|i| i.id != c.id);
        out.push(c.id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }
    fn f(name: &str, size: u64, secs: u64) -> (String, u64, SystemTime) {
        (name.to_owned(), size, t(secs))
    }

    #[test]
    fn only_export_files_are_ever_considered() {
        let files = vec![
            f("incident-1-1.json", 10, 1),
            f("notes.txt", 10_000, 0),
            f("incident-2-2.protected.json", 10, 2),
            f("incident-3.bak", 5, 3),
        ];
        // Limite de 1 arquivo: só o mais antigo das exportações sai; os outros nomes nem entram na conta.
        assert_eq!(exports_to_remove(&files, "incident-2-2.protected.json", 1, u64::MAX), vec!["incident-1-1.json"]);
    }

    #[test]
    fn the_newest_files_are_kept_and_the_oldest_removed() {
        let files: Vec<_> = (1..=5).map(|i| f(&format!("incident-{i}-{i}.json"), 1, i)).collect();
        assert_eq!(exports_to_remove(&files, "incident-5-5.json", 3, u64::MAX), vec!["incident-2-2.json", "incident-1-1.json"]);
    }

    #[test]
    fn the_byte_limit_applies_too_and_the_file_just_written_is_never_removed() {
        let files = vec![f("incident-1-1.json", 60, 1), f("incident-2-2.json", 60, 2), f("incident-3-3.json", 500, 3)];
        // O recém-escrito (3) já passa do limite sozinho, mas nunca é removido; os mais antigos saem.
        let out = exports_to_remove(&files, "incident-3-3.json", 10, 100);
        assert!(!out.contains(&"incident-3-3.json".to_owned()));
        assert_eq!(out, vec!["incident-2-2.json", "incident-1-1.json"]);
    }

    #[test]
    fn under_the_limits_nothing_is_removed() {
        let files = vec![f("incident-1-1.json", 5, 1), f("incident-2-2.json", 5, 2)];
        assert!(exports_to_remove(&files, "incident-2-2.json", 30, 100 << 20).is_empty());
        assert!(exports_to_remove(&[], "", 30, 1).is_empty());
    }

    fn item(id: i64, created: i64, kind: IncidentKind, segs: &[u64]) -> EvidenceItem {
        EvidenceItem {
            id,
            created_utc_ms: created,
            kind,
            state: InvestigationState::New,
            capture: CaptureState::Preserved,
            has_notes: false,
            segments: segs.to_vec(),
        }
    }
    fn sizes(pairs: &[(u64, u64)]) -> HashMap<u64, u64> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn the_oldest_automatic_incidents_go_first_until_the_budget_fits() {
        let items = vec![
            item(1, 100, IncidentKind::CpuSustained, &[1]),
            item(2, 200, IncidentKind::AppHang, &[2]),
            item(3, 300, IncidentKind::Throttling, &[3]),
        ];
        let s = sizes(&[(1, 40), (2, 40), (3, 40)]);
        assert_eq!(evidence_to_remove(&items, &s, 100), vec![1], "120 > 100: only the oldest has to go");
        assert_eq!(evidence_to_remove(&items, &s, 50), vec![1, 2]);
        assert!(evidence_to_remove(&items, &s, 120).is_empty(), "exactly at the budget is fine");
    }

    #[test]
    fn manual_noted_and_worked_on_incidents_are_never_removed() {
        let mut manual = item(1, 100, IncidentKind::Manual, &[1]);
        manual.state = InvestigationState::New;
        let mut noted = item(2, 110, IncidentKind::AppHang, &[2]);
        noted.has_notes = true;
        let mut investigating = item(3, 120, IncidentKind::BlueScreen, &[3]);
        investigating.state = InvestigationState::Investigating;
        let mut resolved = item(4, 130, IncidentKind::CpuSustained, &[4]);
        resolved.state = InvestigationState::Resolved;
        let mut capturing = item(5, 140, IncidentKind::MemoryHigh, &[5]);
        capturing.capture = CaptureState::Capturing;
        let mut dismissed = item(6, 150, IncidentKind::MemoryHigh, &[6]);
        dismissed.state = InvestigationState::Dismissed;
        let s = sizes(&[(1, 50), (2, 50), (3, 50), (4, 50), (5, 50), (6, 50)]);
        let items = vec![manual, noted, investigating, resolved, capturing, dismissed];
        // Estoura o orçamento de longe, mas só o descartado (6) é removível.
        assert_eq!(evidence_to_remove(&items, &s, 0), vec![6]);
    }

    #[test]
    fn a_segment_shared_by_two_incidents_counts_once_and_is_freed_only_when_both_go() {
        let items = vec![item(1, 100, IncidentKind::CpuSustained, &[1, 2]), item(2, 200, IncidentKind::AppHang, &[2, 3])];
        let s = sizes(&[(1, 10), (2, 100), (3, 10)]);
        // Removendo o 1, o segmento 2 continua ocupado pelo 2: 100 + 10 = 110.
        assert_eq!(evidence_bytes(&items.iter().collect::<Vec<_>>(), &s), 120);
        assert_eq!(evidence_to_remove(&items, &s, 119), vec![1]);
        assert_eq!(evidence_to_remove(&items, &s, 100), vec![1, 2], "the shared segment only leaves with the second incident");
    }
}
