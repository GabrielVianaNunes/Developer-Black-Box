//! Detecção simples de throttling térmico: o sinal que a aba "Saúde do sistema" (#73) vai consumir.
//!
//! Há throttling quando o limite passivo do Windows está ABAIXO de 100 % (o sistema está reduzindo o desempenho por
//! calor) enquanto a CPU está com carga alta. Só conta se as DUAS leituras existem: sem dado não há alarme (falta de
//! sinal nunca vira "throttling"). Prolongado = várias amostras seguidas. Puro, sem relógio.

use bb_core::HealthSample;

/// Carga mínima da CPU (%) para o limite passivo baixo valer como throttling.
pub const HIGH_LOAD_PCT: u8 = 70;
/// Quantas amostras seguidas (~30 s cada) contam como throttling prolongado: 10 = ~5 minutos.
pub const SUSTAINED_SAMPLES: u32 = 10;

#[derive(Debug, Default)]
pub struct ThrottleDetector {
    streak: u32,
}

impl ThrottleDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Esquece a sequência (telemetria desligada, pausa).
    pub fn reset(&mut self) {
        self.streak = 0;
    }

    /// Esta amostra, sozinha, indica throttling?
    pub fn is_throttled(s: &HealthSample) -> bool {
        matches!((s.passive_limit_pct, s.cpu_load_pct), (Some(limit), Some(load)) if limit < 100 && load >= HIGH_LOAD_PCT)
    }

    /// Alimenta com a amostra. Devolve `true` quando o throttling já dura `SUSTAINED_SAMPLES` amostras seguidas.
    pub fn observe(&mut self, s: &HealthSample) -> bool {
        if Self::is_throttled(s) {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.streak = 0; // sem throttling, ou sem dado suficiente: a sequência recomeça
        }
        self.sustained()
    }

    pub fn sustained(&self) -> bool {
        self.streak >= SUSTAINED_SAMPLES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(limit: Option<u8>, load: Option<u8>) -> HealthSample {
        HealthSample { passive_limit_pct: limit, cpu_load_pct: load, ..HealthSample::default() }
    }

    #[test]
    fn a_low_passive_limit_under_high_load_is_throttling() {
        assert!(ThrottleDetector::is_throttled(&s(Some(80), Some(95))));
        assert!(ThrottleDetector::is_throttled(&s(Some(99), Some(70))), "boundaries: limit 99 and load 70");
    }

    #[test]
    fn full_limit_or_low_load_is_not_throttling() {
        assert!(!ThrottleDetector::is_throttled(&s(Some(100), Some(100))), "100 means no reduction");
        assert!(!ThrottleDetector::is_throttled(&s(Some(60), Some(69))), "a cool, idle machine can have a low limit");
    }

    #[test]
    fn missing_data_is_never_throttling() {
        assert!(!ThrottleDetector::is_throttled(&s(None, Some(100))));
        assert!(!ThrottleDetector::is_throttled(&s(Some(50), None)));
        assert!(!ThrottleDetector::is_throttled(&HealthSample::default()));
    }

    #[test]
    fn it_becomes_sustained_only_after_enough_consecutive_samples() {
        let mut d = ThrottleDetector::new();
        for i in 1..SUSTAINED_SAMPLES {
            assert!(!d.observe(&s(Some(70), Some(90))), "sample {i}");
        }
        assert!(d.observe(&s(Some(70), Some(90))));
        assert!(d.sustained());
    }

    #[test]
    fn one_good_or_unknown_sample_restarts_the_streak() {
        let mut d = ThrottleDetector::new();
        for _ in 0..SUSTAINED_SAMPLES - 1 {
            d.observe(&s(Some(70), Some(90)));
        }
        assert!(!d.observe(&s(Some(100), Some(90))));
        for _ in 0..SUSTAINED_SAMPLES - 1 {
            assert!(!d.observe(&s(Some(70), Some(90))));
        }
        d.observe(&s(None, Some(90)));
        assert!(!d.sustained(), "a sample without data breaks the streak (no alarm without a signal)");
    }

    #[test]
    fn reset_clears_a_sustained_state() {
        let mut d = ThrottleDetector::new();
        for _ in 0..SUSTAINED_SAMPLES {
            d.observe(&s(Some(70), Some(90)));
        }
        assert!(d.sustained());
        d.reset();
        assert!(!d.sustained());
    }
}
