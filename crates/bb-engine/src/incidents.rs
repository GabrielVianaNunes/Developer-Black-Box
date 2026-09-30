//! Detecção de anomalias de recursos e captura de incidentes.
//!
//! O detector só enxerga eventos já aprovados pelo Guard, então não pode criar
//! um incidente sobre um app protegido, excluído ou coletado durante uma pausa.
//! Um incidente registra fatos ("CPU alta por N amostras"), nunca causas.

use std::collections::HashMap;

use bb_core::{EventKind, ExeName, ProcessKey};
use bb_store::{IncidentKind, Severity};

#[derive(Clone, Debug)]
pub struct IncidentConfig {
    /// Evidência anterior ao incidente que é preservada.
    pub pre_window_ms: i64,
    /// Evidência posterior ao incidente que continua sendo preservada.
    pub post_window_ms: i64,
    pub cpu_threshold_permille: u16,
    /// Amostras de métricas consecutivas acima do limiar para disparar.
    pub cpu_samples: u32,
    pub memory_threshold_kb: u64,
    /// Não repete o mesmo tipo de incidente para o mesmo processo antes disto.
    pub cooldown_ms: i64,
}

impl Default for IncidentConfig {
    fn default() -> Self {
        Self {
            pre_window_ms: 60_000,
            post_window_ms: 30_000,
            cpu_threshold_permille: 900,
            cpu_samples: 3,
            memory_threshold_kb: 4_000_000,
            cooldown_ms: 300_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: IncidentKind,
    pub severity: Severity,
    pub exe_name: ExeName,
    pub summary: String,
}

pub struct Detector {
    cfg: IncidentConfig,
    exe: HashMap<ProcessKey, ExeName>,
    cpu_streak: HashMap<ProcessKey, u32>,
    last_fired: HashMap<(ProcessKey, IncidentKind), i64>,
    /// Falhas/travamentos são por aplicativo (não por instância): um intervalo curto evita rajadas.
    last_app_incident: HashMap<(String, IncidentKind), i64>,
}

/// Intervalo mínimo entre incidentes do mesmo tipo para o mesmo aplicativo (falha/travamento).
const APP_INCIDENT_COOLDOWN_MS: i64 = 60_000;

impl Detector {
    pub fn new(cfg: IncidentConfig) -> Self {
        Self {
            cfg,
            exe: HashMap::new(),
            cpu_streak: HashMap::new(),
            last_fired: HashMap::new(),
            last_app_incident: HashMap::new(),
        }
    }

    fn app_cooled_down(&mut self, exe: &ExeName, kind: IncidentKind, now: i64) -> bool {
        let e = self.last_app_incident.entry((exe.as_str().to_owned(), kind)).or_insert(i64::MIN);
        if *e == i64::MIN || now - *e >= APP_INCIDENT_COOLDOWN_MS {
            *e = now;
            true
        } else {
            false
        }
    }

    pub fn config(&self) -> &IncidentConfig {
        &self.cfg
    }

    /// Alimentado com cada evento admitido pelo Guard, na ordem.
    pub fn observe(&mut self, kind: &EventKind, ts_utc_ms: i64) -> Option<Finding> {
        match kind {
            EventKind::ProcessStarted { key, exe_name, .. } => {
                self.exe.insert(*key, exe_name.clone());
                None
            }
            EventKind::AppCrash { exe_name, exception_code } => {
                if !self.app_cooled_down(exe_name, IncidentKind::UnexpectedExit, ts_utc_ms) {
                    return None;
                }
                Some(Finding {
                    kind: IncidentKind::UnexpectedExit,
                    severity: Severity::Critical,
                    exe_name: exe_name.clone(),
                    summary: format!("Aplicativo encerrado com erro (código de exceção 0x{exception_code:08X})"),
                })
            }
            EventKind::AppHang { exe_name } => {
                if !self.app_cooled_down(exe_name, IncidentKind::AppHang, ts_utc_ms) {
                    return None;
                }
                Some(Finding {
                    kind: IncidentKind::AppHang,
                    severity: Severity::Warning,
                    exe_name: exe_name.clone(),
                    summary: "Aplicativo deixou de responder".into(),
                })
            }
            EventKind::ProcessExited { key, .. } => {
                self.exe.remove(key);
                self.cpu_streak.remove(key);
                self.last_fired.retain(|(k, _), _| k != key);
                None
            }
            EventKind::ProcessMetrics { key, cpu_permille, working_set_kb } => {
                let exe = self.exe.get(key)?.clone();
                let streak = self.cpu_streak.entry(*key).or_insert(0);
                if *cpu_permille >= self.cfg.cpu_threshold_permille {
                    *streak += 1;
                } else {
                    *streak = 0;
                }
                let streak = *streak;

                if streak >= self.cfg.cpu_samples && self.cooled_down(*key, IncidentKind::CpuSustained, ts_utc_ms) {
                    return Some(Finding {
                        kind: IncidentKind::CpuSustained,
                        severity: Severity::Warning,
                        exe_name: exe,
                        summary: format!(
                            "CPU em {},{}% ou mais por {} amostras consecutivas",
                            self.cfg.cpu_threshold_permille / 10,
                            self.cfg.cpu_threshold_permille % 10,
                            streak
                        ),
                    });
                }
                if *working_set_kb >= self.cfg.memory_threshold_kb
                    && self.cooled_down(*key, IncidentKind::MemoryHigh, ts_utc_ms)
                {
                    return Some(Finding {
                        kind: IncidentKind::MemoryHigh,
                        severity: Severity::Warning,
                        exe_name: exe,
                        summary: format!("Memória em uso de {} MB ou mais", self.cfg.memory_threshold_kb / 1024),
                    });
                }
                None
            }
            _ => None,
        }
    }

    fn cooled_down(&mut self, key: ProcessKey, kind: IncidentKind, now: i64) -> bool {
        let entry = self.last_fired.entry((key, kind)).or_insert(i64::MIN);
        if *entry == i64::MIN || now - *entry >= self.cfg.cooldown_ms {
            *entry = now;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pid: u32) -> ProcessKey {
        ProcessKey { pid, start_time_ms: 1 }
    }
    fn started(pid: u32) -> EventKind {
        EventKind::ProcessStarted { key: key(pid), exe_name: ExeName::new("synth.exe").unwrap(), parent_pid: 0 }
    }
    fn m(pid: u32, cpu: u16, ws: u64) -> EventKind {
        EventKind::ProcessMetrics { key: key(pid), cpu_permille: cpu, working_set_kb: ws }
    }
    fn cfg() -> IncidentConfig {
        IncidentConfig { cpu_samples: 3, cooldown_ms: 1_000, ..IncidentConfig::default() }
    }

    #[test]
    fn sustained_cpu_fires_only_after_enough_consecutive_samples() {
        let mut d = Detector::new(cfg());
        d.observe(&started(1), 0);
        assert!(d.observe(&m(1, 950, 1), 1).is_none());
        assert!(d.observe(&m(1, 950, 1), 2).is_none());
        let f = d.observe(&m(1, 950, 1), 3).expect("third sample fires");
        assert_eq!((f.kind, f.severity), (IncidentKind::CpuSustained, Severity::Warning));
    }

    #[test]
    fn a_dip_resets_the_streak() {
        let mut d = Detector::new(cfg());
        d.observe(&started(1), 0);
        d.observe(&m(1, 950, 1), 1);
        d.observe(&m(1, 950, 1), 2);
        d.observe(&m(1, 100, 1), 3);
        assert!(d.observe(&m(1, 950, 1), 4).is_none());
    }

    #[test]
    fn cooldown_suppresses_repeats_then_allows_again() {
        let mut d = Detector::new(cfg());
        d.observe(&started(1), 0);
        for t in 1..=3 {
            d.observe(&m(1, 950, 1), t);
        }
        assert!(d.observe(&m(1, 950, 1), 500).is_none());
        assert!(d.observe(&m(1, 950, 1), 1_500).is_some());
    }

    #[test]
    fn high_memory_fires_once_per_cooldown() {
        let mut d = Detector::new(cfg());
        d.observe(&started(1), 0);
        let f = d.observe(&m(1, 0, 5_000_000), 1).unwrap();
        assert_eq!(f.kind, IncidentKind::MemoryHigh);
        assert!(d.observe(&m(1, 0, 5_000_000), 2).is_none());
    }

    #[test]
    fn a_crash_opens_a_critical_incident_stating_only_the_exception_code() {
        let mut d = Detector::new(cfg());
        let f = d
            .observe(&EventKind::AppCrash { exe_name: ExeName::new("synth.exe").unwrap(), exception_code: 0xc000_0005 }, 10)
            .unwrap();
        assert_eq!((f.kind, f.severity), (IncidentKind::UnexpectedExit, Severity::Critical));
        assert!(f.summary.contains("0xC0000005"));
        assert!(!f.summary.contains("synth"));
    }

    #[test]
    fn a_hang_opens_a_warning_incident() {
        let mut d = Detector::new(cfg());
        let f = d.observe(&EventKind::AppHang { exe_name: ExeName::new("synth.exe").unwrap() }, 10).unwrap();
        assert_eq!((f.kind, f.severity), (IncidentKind::AppHang, Severity::Warning));
    }

    #[test]
    fn crash_bursts_of_the_same_app_are_collapsed_but_other_apps_are_not() {
        let mut d = Detector::new(cfg());
        let crash = |n: &str| EventKind::AppCrash { exe_name: ExeName::new(n).unwrap(), exception_code: 1 };
        assert!(d.observe(&crash("a.exe"), 0).is_some());
        assert!(d.observe(&crash("a.exe"), 5_000).is_none());
        assert!(d.observe(&crash("b.exe"), 5_000).is_some());
        assert!(d.observe(&crash("a.exe"), 61_000).is_some());
    }

    #[test]
    fn unknown_processes_never_produce_findings() {
        // O Guard não admitiu o início: o detector nunca soube quem é.
        let mut d = Detector::new(cfg());
        for t in 0..10 {
            assert!(d.observe(&m(9, 999, 9_999_999), t).is_none());
        }
    }

    #[test]
    fn summary_contains_no_process_or_user_text() {
        let mut d = Detector::new(cfg());
        d.observe(&started(1), 0);
        d.observe(&m(1, 950, 1), 1);
        d.observe(&m(1, 950, 1), 2);
        let f = d.observe(&m(1, 950, 1), 3).unwrap();
        assert!(!f.summary.contains("synth"));
    }
}
