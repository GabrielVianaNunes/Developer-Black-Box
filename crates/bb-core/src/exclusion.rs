//! Tipos de evento que uma regra de exclusão pode cobrir, por programa.
//!
//! Excluir um programa por inteiro continua sendo `GuardConfig::excluded_apps`. Uma exclusão PARCIAL
//! (`partial_exclusions`) diz quais tipos de evento daquele programa NÃO são gravados; o resto continua sendo.
//! Uma regra só pode estreitar o que é gravado de um programa que o usuário escolheu excluir: o padrão ao criar
//! uma regra é excluir tudo, e gravar algo é uma escolha explícita.

/// Os três tipos de evento de um programa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExclusionKind {
    /// Início e fim do processo (`ProcessStarted`, `ProcessExited`). É o que dá o NOME ao programa nos dados.
    Lifecycle,
    /// Uso de CPU e memória do processo (`ProcessMetrics`). Sem o início gravado não há como saber de quem é.
    Metrics,
    /// Falhas e travamentos (`AppCrash`, `AppHang`).
    Crashes,
}

impl ExclusionKind {
    pub const ALL: [ExclusionKind; 3] = [ExclusionKind::Lifecycle, ExclusionKind::Metrics, ExclusionKind::Crashes];

    /// Código estável (gravado nas configurações e enviado à interface).
    pub fn code(self) -> &'static str {
        match self {
            ExclusionKind::Lifecycle => "lifecycle",
            ExclusionKind::Metrics => "metrics",
            ExclusionKind::Crashes => "crashes",
        }
    }

    pub fn parse(code: &str) -> Option<ExclusionKind> {
        ExclusionKind::ALL.into_iter().find(|k| k.code() == code)
    }
}

/// Conjunto de tipos EXCLUÍDOS de um programa. Sempre coerente: excluir o início/fim exclui também CPU e memória
/// (métricas sem início gravado ficariam sem identidade), então a regra "métricas gravadas só com início gravado"
/// vale em qualquer construção, inclusive a partir de dados editados à mão. Em dúvida, exclui mais.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExclusionSet {
    lifecycle: bool,
    metrics: bool,
    crashes: bool,
}

impl ExclusionSet {
    /// Nada excluído (a regra não faz nada).
    pub const NONE: ExclusionSet = ExclusionSet { lifecycle: false, metrics: false, crashes: false };
    /// Tudo excluído: o mesmo que excluir o programa por inteiro.
    pub const ALL: ExclusionSet = ExclusionSet { lifecycle: true, metrics: true, crashes: true };

    /// Monta o conjunto aplicando a coerência (início/fim excluído leva CPU e memória junto).
    pub fn new(lifecycle: bool, metrics: bool, crashes: bool) -> Self {
        ExclusionSet { lifecycle, metrics: metrics || lifecycle, crashes }
    }

    pub fn from_kinds(kinds: impl IntoIterator<Item = ExclusionKind>) -> Self {
        let (mut l, mut m, mut c) = (false, false, false);
        for k in kinds {
            match k {
                ExclusionKind::Lifecycle => l = true,
                ExclusionKind::Metrics => m = true,
                ExclusionKind::Crashes => c = true,
            }
        }
        ExclusionSet::new(l, m, c)
    }

    pub fn contains(&self, kind: ExclusionKind) -> bool {
        match kind {
            ExclusionKind::Lifecycle => self.lifecycle,
            ExclusionKind::Metrics => self.metrics || self.lifecycle, // coerência também na leitura
            ExclusionKind::Crashes => self.crashes,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.contains(ExclusionKind::Lifecycle) && !self.contains(ExclusionKind::Metrics) && !self.contains(ExclusionKind::Crashes)
    }

    /// Cobre tudo: equivale a excluir o programa por inteiro.
    pub fn is_all(&self) -> bool {
        ExclusionKind::ALL.iter().all(|k| self.contains(*k))
    }

    /// União: o conjunto que exclui o que qualquer um dos dois exclui (o lado mais privado).
    pub fn union(self, other: ExclusionSet) -> ExclusionSet {
        ExclusionSet::new(
            self.contains(ExclusionKind::Lifecycle) || other.contains(ExclusionKind::Lifecycle),
            self.contains(ExclusionKind::Metrics) || other.contains(ExclusionKind::Metrics),
            self.contains(ExclusionKind::Crashes) || other.contains(ExclusionKind::Crashes),
        )
    }

    /// Tipos excluídos, em ordem fixa.
    pub fn kinds(&self) -> Vec<ExclusionKind> {
        ExclusionKind::ALL.into_iter().filter(|k| self.contains(*k)).collect()
    }

    /// Códigos separados por vírgula (`lifecycle,metrics`), como gravados nas configurações.
    pub fn to_codes(&self) -> String {
        self.kinds().iter().map(|k| k.code()).collect::<Vec<_>>().join(",")
    }

    /// Lê os códigos. FALHA FECHADA: texto vazio ou com qualquer código desconhecido vira exclusão TOTAL
    /// (um valor ilegível nunca pode deixar um programa menos excluído do que o usuário queria).
    pub fn from_codes(text: &str) -> ExclusionSet {
        let parts: Vec<&str> = text.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return ExclusionSet::ALL;
        }
        let kinds: Option<Vec<ExclusionKind>> = parts.iter().map(|p| ExclusionKind::parse(p)).collect();
        match kinds {
            Some(k) => ExclusionSet::from_kinds(k),
            None => ExclusionSet::ALL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ExclusionKind::*;

    #[test]
    fn excluding_the_lifecycle_always_excludes_cpu_and_memory_too() {
        let s = ExclusionSet::new(true, false, false);
        assert!(s.contains(Lifecycle) && s.contains(Metrics) && !s.contains(Crashes));
        assert_eq!(s.kinds(), vec![Lifecycle, Metrics]);
        assert_eq!(ExclusionSet::from_kinds([Lifecycle]).kinds(), vec![Lifecycle, Metrics]);
    }

    #[test]
    fn metrics_alone_is_a_valid_set_and_all_is_all() {
        let s = ExclusionSet::new(false, true, false);
        assert_eq!(s.kinds(), vec![Metrics]);
        assert!(!s.is_all() && !s.is_empty());
        assert!(ExclusionSet::ALL.is_all() && ExclusionSet::NONE.is_empty());
        assert!(ExclusionSet::new(true, true, true).is_all());
    }

    #[test]
    fn union_takes_the_more_private_side() {
        let a = ExclusionSet::new(false, true, false);
        let b = ExclusionSet::new(false, false, true);
        assert_eq!(a.union(b).kinds(), vec![Metrics, Crashes]);
        assert!(ExclusionSet::NONE.union(ExclusionSet::ALL).is_all());
    }

    #[test]
    fn codes_round_trip_for_every_coherent_set() {
        for l in [false, true] {
            for m in [false, true] {
                for c in [false, true] {
                    let s = ExclusionSet::new(l, m, c);
                    if s.is_empty() {
                        continue; // sem tipos não há o que gravar
                    }
                    assert_eq!(ExclusionSet::from_codes(&s.to_codes()), s, "{l} {m} {c}");
                }
            }
        }
    }

    #[test]
    fn unreadable_codes_fail_closed_to_full_exclusion() {
        for bad in ["", "  ", ",", "bogus", "lifecycle,bogus", "metrics;crashes", "LIFECYCLE"] {
            assert!(ExclusionSet::from_codes(bad).is_all(), "{bad:?}");
        }
        assert_eq!(ExclusionSet::from_codes(" metrics , crashes ").kinds(), vec![Metrics, Crashes], "spaces are fine");
    }

    #[test]
    fn codes_are_stable_and_distinct() {
        let codes: Vec<&str> = ExclusionKind::ALL.iter().map(|k| k.code()).collect();
        assert_eq!(codes, ["lifecycle", "metrics", "crashes"]);
        for k in ExclusionKind::ALL {
            assert_eq!(ExclusionKind::parse(k.code()), Some(k));
        }
    }
}
