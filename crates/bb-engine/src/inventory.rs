//! Comparação do inventário com a leitura anterior: só MUDANÇAS viram evento.
//!
//! Lógica pura (sem Windows, sem armazenamento). Regras:
//! - Primeira vez que um item é lido: só vira referência, nunca evento.
//! - Item não lido agora (fonte indisponível): sem evento e a referência anterior é mantida. Indisponível não é mudança.
//! - Valor diferente da referência: um evento com o valor anterior e o novo, e a referência passa a ser o novo.
//! A referência guarda só números por item (nenhum identificador).

use bb_collector::inventory::ALL;
use bb_collector::Snapshot;
use bb_core::InventoryItem;

/// Referência: o último valor conhecido de cada item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Baseline(Vec<(InventoryItem, u64)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub item: InventoryItem,
    pub previous: Option<u64>,
    pub current: Option<u64>,
}

fn key(item: InventoryItem) -> &'static str {
    match item {
        InventoryItem::BiosVersion => "bios_version",
        InventoryItem::BiosDate => "bios_date",
        InventoryItem::FirmwareType => "firmware",
        InventoryItem::SecureBoot => "secure_boot",
        InventoryItem::OsBuild => "os_build",
        InventoryItem::DeviceProblemCount => "problem_count",
        InventoryItem::DeviceProblemCodes => "problem_codes",
    }
}

impl Baseline {
    pub fn get(&self, item: InventoryItem) -> Option<u64> {
        self.0.iter().find(|(i, _)| *i == item).map(|(_, v)| *v)
    }

    fn set(&mut self, item: InventoryItem, v: u64) {
        match self.0.iter_mut().find(|(i, _)| *i == item) {
            Some(slot) => slot.1 = v,
            None => self.0.push((item, v)),
        }
    }

    /// "chave=número;chave=número". Itens desconhecidos e valores inválidos são ignorados (referência corrompida =
    /// itens tratados como "primeira leitura", sem alarme).
    pub fn parse(s: &str) -> Self {
        let mut b = Baseline::default();
        for part in s.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            let (Some(item), Ok(v)) = (ALL.iter().copied().find(|i| key(*i) == k.trim()), v.trim().parse::<u64>()) else { continue };
            b.set(item, v);
        }
        b
    }

    pub fn serialize(&self) -> String {
        ALL.iter().filter_map(|i| self.get(*i).map(|v| format!("{}={v}", key(*i)))).collect::<Vec<_>>().join(";")
    }
}

/// Compara a leitura com a referência. Devolve as mudanças e a nova referência.
pub fn compare(baseline: &Baseline, snap: &Snapshot) -> (Vec<Change>, Baseline) {
    let mut next = baseline.clone();
    let mut changes = Vec::new();
    for item in ALL {
        let Some(current) = snap.get(item) else { continue };
        match baseline.get(item) {
            Some(previous) if previous != current => {
                changes.push(Change { item, previous: Some(previous), current: Some(current) });
                next.set(item, current);
            }
            Some(_) => {}
            None => next.set(item, current),
        }
    }
    (changes, next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use InventoryItem::*;

    fn snap(v: &[(InventoryItem, Option<u64>)]) -> Snapshot {
        Snapshot(v.to_vec())
    }

    #[test]
    fn the_first_reading_only_sets_the_baseline() {
        let (changes, next) = compare(&Baseline::default(), &snap(&[(OsBuild, Some(10)), (SecureBoot, Some(1))]));
        assert!(changes.is_empty());
        assert_eq!((next.get(OsBuild), next.get(SecureBoot)), (Some(10), Some(1)));
    }

    #[test]
    fn a_changed_value_is_reported_with_previous_and_new() {
        let base = Baseline::parse("os_build=10;secure_boot=1");
        let (changes, next) = compare(&base, &snap(&[(OsBuild, Some(11)), (SecureBoot, Some(1))]));
        assert_eq!(changes, vec![Change { item: OsBuild, previous: Some(10), current: Some(11) }]);
        assert_eq!(next.get(OsBuild), Some(11));
    }

    #[test]
    fn an_unchanged_reading_reports_nothing_and_the_same_change_is_not_reported_twice() {
        let base = Baseline::parse("os_build=10");
        let (c1, b1) = compare(&base, &snap(&[(OsBuild, Some(11))]));
        assert_eq!(c1.len(), 1);
        let (c2, _) = compare(&b1, &snap(&[(OsBuild, Some(11))]));
        assert!(c2.is_empty());
    }

    #[test]
    fn an_unavailable_item_is_not_a_change_and_keeps_the_baseline() {
        let base = Baseline::parse("secure_boot=1;os_build=10");
        let (changes, next) = compare(&base, &snap(&[(SecureBoot, None), (OsBuild, None)]));
        assert!(changes.is_empty());
        assert_eq!(next, base);
        // ao voltar a ler o mesmo valor, ainda nenhum alarme
        let (changes, _) = compare(&next, &snap(&[(SecureBoot, Some(1))]));
        assert!(changes.is_empty());
    }

    #[test]
    fn a_new_failing_device_shows_as_count_and_code_changes() {
        let base = Baseline::parse("problem_count=0;problem_codes=0");
        let (changes, _) = compare(&base, &snap(&[(DeviceProblemCount, Some(1)), (DeviceProblemCodes, Some(1 << 10))]));
        assert_eq!(
            changes,
            vec![
                Change { item: DeviceProblemCount, previous: Some(0), current: Some(1) },
                Change { item: DeviceProblemCodes, previous: Some(0), current: Some(1 << 10) },
            ]
        );
    }

    #[test]
    fn the_baseline_round_trips_and_ignores_garbage() {
        let mut b = Baseline::default();
        b.set(BiosVersion, (1 << 52) | 7);
        b.set(OsBuild, 42);
        assert_eq!(Baseline::parse(&b.serialize()), b);
        let junk = Baseline::parse("nonsense;os_build=abc;;unknown=5;bios_date=20240307;=;secure_boot=-1");
        assert_eq!(junk.get(BiosDate), Some(20240307));
        assert_eq!((junk.get(OsBuild), junk.get(SecureBoot)), (None, None));
    }

    #[test]
    fn the_serialized_baseline_holds_only_known_keys_and_numbers() {
        let mut b = Baseline::default();
        for (i, item) in ALL.iter().enumerate() {
            b.set(*item, i as u64);
        }
        let s = b.serialize();
        assert!(s.split(';').all(|p| {
            let (k, v) = p.split_once('=').unwrap();
            ALL.iter().any(|i| key(*i) == k) && v.chars().all(|c| c.is_ascii_digit())
        }));
    }
}
