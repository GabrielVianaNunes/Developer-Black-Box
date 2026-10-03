//! Inventário da máquina (BIOS/firmware, Secure Boot, build do Windows, dispositivos com problema de driver).
//!
//! Tudo vira NÚMERO. Nada de número de série, UUID, nome do computador, modelo, fabricante ou nome de dispositivo:
//! este módulo nem tem onde guardá-los. Puro (sem Windows) para ser testado com leituras sintéticas.

use bb_core::InventoryItem;

use crate::sample::CollectError;

/// Todos os itens, na ordem em que são lidos e gravados.
pub const ALL: [InventoryItem; 7] = [
    InventoryItem::BiosVersion,
    InventoryItem::BiosDate,
    InventoryItem::FirmwareType,
    InventoryItem::SecureBoot,
    InventoryItem::OsBuild,
    InventoryItem::DeviceProblemCount,
    InventoryItem::DeviceProblemCodes,
];

/// Marca de "formato não numérico": o valor é um hash de 52 bits da versão, só serve para detectar mudança.
/// Todos os valores ficam abaixo de 2^53, para a interface (JavaScript) não perder precisão.
pub const HASHED_FLAG: u64 = 1 << 52;
const COMPONENT_BITS: u32 = 13;
const COMPONENT_MAX: u64 = (1 << COMPONENT_BITS) - 1;

/// Uma leitura do inventário: um valor por item; `None` = não foi possível ler (indisponível).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Snapshot(pub Vec<(InventoryItem, Option<u64>)>);

impl Snapshot {
    pub fn get(&self, item: InventoryItem) -> Option<u64> {
        self.0.iter().find(|(i, _)| *i == item).and_then(|(_, v)| *v)
    }
}

/// Fonte do inventário. Erro = fonte indisponível (o motor segue sem ela, sem alarme).
pub trait InventorySource {
    fn read(&mut self) -> Result<Snapshot, CollectError>;
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// "1.2.3" -> 1,2,3 empacotados (até 4 componentes de 13 bits). Qualquer outra forma (letras, componente grande,
/// vazio, mais de 4 componentes) vira um código de hash com `HASHED_FLAG`. Vazio = `None` (nada lido).
pub fn version_code(raw: &str) -> Option<u64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let parts: Vec<&str> = t.split('.').collect();
    if parts.len() <= 4 {
        let nums: Option<Vec<u64>> = parts
            .iter()
            .map(|p| if !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) { p.parse::<u64>().ok() } else { None })
            .collect();
        if let Some(nums) = nums.filter(|n| n.iter().all(|c| *c <= COMPONENT_MAX)) {
            let mut v = 0u64;
            for i in 0..4 {
                v = (v << COMPONENT_BITS) | nums.get(i).copied().unwrap_or(0);
            }
            return Some(v);
        }
    }
    Some(HASHED_FLAG | (fnv1a(&t.to_lowercase()) & (HASHED_FLAG - 1)))
}

/// Inverso de `version_code` para exibir; `None` se o valor é um hash (formato não numérico).
pub fn unpack_version(v: u64) -> Option<[u16; 4]> {
    if v >= HASHED_FLAG {
        return None;
    }
    let mut out = [0u16; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = ((v >> (COMPONENT_BITS * (3 - i as u32))) & COMPONENT_MAX) as u16;
    }
    Some(out)
}

/// "MM/DD/AAAA" (formato do registro do Windows) -> AAAAMMDD. Outra forma ou data impossível = `None`.
pub fn bios_date_code(raw: &str) -> Option<u64> {
    let p: Vec<&str> = raw.trim().split('/').collect();
    let [m, d, y] = p.as_slice() else { return None };
    let (m, d, y): (u64, u64, u64) = (m.parse().ok()?, d.parse().ok()?, y.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d) && (1980..=2200).contains(&y) && y.to_string().len() == 4)
        .then_some(y * 10_000 + m * 100 + d)
}

/// Build do Windows e revisão (UBR) num número só.
pub fn os_build_code(build: u32, revision: u32) -> Option<u64> {
    (build > 0 && build < (1 << 20) && revision < (1 << 20)).then(|| (u64::from(build) << 20) | u64::from(revision))
}

pub fn unpack_os_build(v: u64) -> (u32, u32) {
    ((v >> 20) as u32, (v & 0xF_FFFF) as u32)
}

/// Código "dispositivo desativado pelo usuário" (CM_PROB_DISABLED): escolha da pessoa, não falha.
pub const PROBLEM_DISABLED: u32 = 22;

/// Códigos de problema de driver (CM_PROB_*) dos dispositivos presentes -> (quantos, máscara de códigos). O código 0
/// (sem problema) e o 22 (desativado de propósito) não contam; códigos acima de 52 caem no bit 0 ("outro"). A máscara
/// fica abaixo de 2^53.
pub fn problem_summary(codes: &[u32]) -> (u64, u64) {
    let mut count = 0u64;
    let mut mask = 0u64;
    for &c in codes.iter().filter(|c| **c != 0 && **c != PROBLEM_DISABLED) {
        count += 1;
        mask |= 1u64 << if c <= 52 { c } else { 0 };
    }
    (count, mask)
}

/// Monta a leitura a partir de valores já lidos (cada um `None` se a fonte não o deu).
pub fn snapshot(
    bios_version: Option<&str>,
    bios_date: Option<&str>,
    firmware: Option<u64>,
    secure_boot: Option<bool>,
    os: Option<(u32, u32)>,
    problem_codes: Option<&[u32]>,
) -> Snapshot {
    let summary = problem_codes.map(problem_summary);
    Snapshot(vec![
        (InventoryItem::BiosVersion, bios_version.and_then(version_code)),
        (InventoryItem::BiosDate, bios_date.and_then(bios_date_code)),
        (InventoryItem::FirmwareType, firmware.filter(|f| matches!(f, 1 | 2))),
        (InventoryItem::SecureBoot, secure_boot.map(u64::from)),
        (InventoryItem::OsBuild, os.and_then(|(b, r)| os_build_code(b, r))),
        (InventoryItem::DeviceProblemCount, summary.map(|s| s.0)),
        (InventoryItem::DeviceProblemCodes, summary.map(|s| s.1)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_versions_are_packed_and_round_trip() {
        let v = version_code("1.22.333").unwrap();
        assert!(v < HASHED_FLAG);
        assert_eq!(unpack_version(v), Some([1, 22, 333, 0]));
        assert_eq!(unpack_version(version_code("7").unwrap()), Some([7, 0, 0, 0]));
        assert_eq!(unpack_version(version_code("8191.1.2.3").unwrap()), Some([8191, 1, 2, 3]));
    }

    #[test]
    fn different_versions_give_different_codes_and_the_same_version_the_same() {
        assert_ne!(version_code("1.2.3"), version_code("1.2.4"));
        assert_eq!(version_code(" 1.2.3 "), version_code("1.2.3"));
    }

    #[test]
    fn non_numeric_versions_become_a_flagged_hash_without_the_text() {
        let a = version_code("F.12").unwrap();
        let b = version_code("F.13").unwrap();
        assert!(a >= HASHED_FLAG && b >= HASHED_FLAG && a < (1 << 53));
        assert_ne!(a, b);
        assert_eq!(unpack_version(a), None);
        assert_eq!(version_code("f.12"), version_code("F.12"), "case does not matter");
        // componente grande demais, componentes demais, vazios no meio: também viram hash
        for odd in ["1.2.3.4.5", "9999.1", "1..2", "v1", "1.2.x"] {
            assert!(version_code(odd).unwrap() >= HASHED_FLAG, "{odd}");
        }
    }

    #[test]
    fn empty_versions_are_not_read() {
        assert_eq!(version_code(""), None);
        assert_eq!(version_code("   "), None);
    }

    #[test]
    fn bios_dates_are_validated() {
        assert_eq!(bios_date_code("03/07/2024"), Some(2024_03_07));
        assert_eq!(bios_date_code(" 12/31/2019 "), Some(2019_12_31));
        for bad in ["", "2024-03-07", "13/01/2024", "00/10/2024", "03/32/2024", "03/07/24", "a/b/c", "03/07/2024/1"] {
            assert_eq!(bios_date_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn os_build_packs_build_and_revision() {
        let v = os_build_code(26100, 1742).unwrap();
        assert_eq!(unpack_os_build(v), (26100, 1742));
        assert!(v < (1 << 53));
        assert_eq!(os_build_code(0, 1), None);
        assert_eq!(os_build_code(1 << 20, 0), None);
    }

    #[test]
    fn problem_codes_are_counted_and_masked() {
        assert_eq!(problem_summary(&[]), (0, 0));
        assert_eq!(problem_summary(&[0, 0]), (0, 0), "code 0 is not a problem");
        assert_eq!(problem_summary(&[22, 22]), (0, 0), "a device the user disabled is not a failure");
        assert_eq!(problem_summary(&[10, 43, 10]), (3, (1 << 10) | (1 << 43)));
        let (n, mask) = problem_summary(&[52, 53, 9999]);
        assert_eq!(n, 3);
        assert_eq!(mask, (1 << 52) | 1, "codes above 52 fall into bit 0");
        assert!(mask < (1 << 53));
    }

    #[test]
    fn the_snapshot_has_one_value_per_item_and_unreadable_ones_are_none() {
        let s = snapshot(Some("1.2.3"), Some("03/07/2024"), Some(2), Some(true), Some((26100, 5)), Some(&[10]));
        assert_eq!(s.0.iter().map(|(i, _)| *i).collect::<Vec<_>>(), ALL.to_vec());
        assert_eq!(s.get(InventoryItem::SecureBoot), Some(1));
        assert_eq!(s.get(InventoryItem::DeviceProblemCount), Some(1));
        let empty = snapshot(None, None, None, None, None, None);
        assert!(ALL.iter().all(|i| empty.get(*i).is_none()));
        // firmware fora de {1, 2} (ex. "desconhecido") não é um valor
        assert_eq!(snapshot(None, None, Some(0), None, None, None).get(InventoryItem::FirmwareType), None);
    }
}
