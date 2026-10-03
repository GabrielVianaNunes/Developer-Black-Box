//! Detecção de anomalias de recursos e captura de incidentes.
//!
//! O detector só enxerga eventos já aprovados pelo Guard, então não pode criar
//! um incidente sobre um app protegido, excluído ou coletado durante uma pausa.
//! Um incidente registra fatos ("CPU alta por N amostras"), nunca causas.
//!
//! O resumo é um CÓDIGO com parâmetros numéricos (`cpu_sustained|900|3`), sem texto de idioma:
//! a interface o traduz ao exibir, então trocar o idioma também traduz incidentes já gravados.

use std::collections::HashMap;

use bb_core::{EventKind, ExeName, HealthCategory, ProcessKey};
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
    /// Só os incidentes de um aplicativo têm nome; os de saúde da máquina (tela azul, desligamento, hardware,
    /// throttling) não citam programa nenhum.
    pub exe_name: Option<ExeName>,
    pub summary: String,
}

pub struct Detector {
    cfg: IncidentConfig,
    exe: HashMap<ProcessKey, ExeName>,
    cpu_streak: HashMap<ProcessKey, u32>,
    last_fired: HashMap<(ProcessKey, IncidentKind), i64>,
    /// Falhas/travamentos são por aplicativo (não por instância): um intervalo curto evita rajadas.
    last_app_incident: HashMap<(String, IncidentKind), i64>,
    /// Incidentes de saúde da máquina: um por grupo (queda de energia/tela azul, hardware, throttling) por intervalo.
    last_health_incident: HashMap<IncidentKind, i64>,
}

/// Dois eventos do mesmo desligamento ruim (ex. Kernel-Power 41 e o 1001 da tela azul) chegam juntos no boot seguinte:
/// um só incidente por grupo neste intervalo.
const HEALTH_INCIDENT_COOLDOWN_MS: i64 = 300_000;
/// Throttling prolongado é uma condição que dura: um aviso por meia hora, não um por amostra.
const THROTTLING_INCIDENT_COOLDOWN_MS: i64 = 30 * 60_000;

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
            last_health_incident: HashMap::new(),
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

    fn health_cooled_down(&mut self, group: IncidentKind, now: i64, cooldown: i64) -> bool {
        let e = self.last_health_incident.entry(group).or_insert(i64::MIN);
        if *e == i64::MIN || now - *e >= cooldown {
            *e = now;
            true
        } else {
            false
        }
    }

    /// Incidente de throttling prolongado: o motor chama quando a condição passa a valer (borda), com os dois números
    /// da última amostra. Respeita o intervalo de meia hora.
    pub fn throttling_finding(&mut self, passive_limit_pct: Option<u8>, cpu_load_pct: Option<u8>, ts_utc_ms: i64) -> Option<Finding> {
        if !self.health_cooled_down(IncidentKind::Throttling, ts_utc_ms, THROTTLING_INCIDENT_COOLDOWN_MS) {
            return None;
        }
        Some(Finding {
            kind: IncidentKind::Throttling,
            severity: Severity::Warning,
            exe_name: None,
            summary: format!("throttling|{}|{}", passive_limit_pct.unwrap_or(0), cpu_load_pct.unwrap_or(0)),
        })
    }

    /// Alimentado com cada evento admitido pelo Guard, na ordem.
    pub fn observe(&mut self, kind: &EventKind, ts_utc_ms: i64) -> Option<Finding> {
        match kind {
            EventKind::ProcessStarted { key, exe_name, .. } => {
                self.exe.insert(*key, exe_name.clone());
                None
            }
            EventKind::HealthEvent { category, event_id, code } => {
                // Kernel-Power 41 com código de verificação diferente de zero É uma tela azul; sem código é falta de energia.
                let (kind, summary) = match category {
                    HealthCategory::UnexpectedShutdown if code.is_some_and(|c| c != 0) => {
                        (IncidentKind::BlueScreen, format!("blue_screen|{}", code.unwrap_or(0)))
                    }
                    HealthCategory::UnexpectedShutdown => (IncidentKind::UnexpectedShutdown, format!("unexpected_shutdown|{event_id}")),
                    HealthCategory::BugCheck => (IncidentKind::BlueScreen, format!("blue_screen|{}", code.unwrap_or(0))),
                    HealthCategory::HardwareError => (IncidentKind::HardwareError, format!("hardware_error|{event_id}")),
                    _ => return None,
                };
                // Falta de energia e tela azul são o mesmo desligamento ruim: um incidente só.
                let group = if kind == IncidentKind::UnexpectedShutdown { IncidentKind::BlueScreen } else { kind };
                if !self.health_cooled_down(group, ts_utc_ms, HEALTH_INCIDENT_COOLDOWN_MS) {
                    return None;
                }
                Some(Finding { kind, severity: Severity::Critical, exe_name: None, summary })
            }
            EventKind::AppCrash { exe_name, exception_code } => {
                if !self.app_cooled_down(exe_name, IncidentKind::UnexpectedExit, ts_utc_ms) {
                    return None;
                }
                Some(Finding {
                    kind: IncidentKind::UnexpectedExit,
                    severity: Severity::Critical,
                    exe_name: Some(exe_name.clone()),
                    summary: format!("app_crash|{exception_code}"),
                })
            }
            EventKind::AppHang { exe_name } => {
                if !self.app_cooled_down(exe_name, IncidentKind::AppHang, ts_utc_ms) {
                    return None;
                }
                Some(Finding {
                    kind: IncidentKind::AppHang,
                    severity: Severity::Warning,
                    exe_name: Some(exe_name.clone()),
                    summary: "app_hang".into(),
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
                        exe_name: Some(exe),
                        summary: format!("cpu_sustained|{}|{}", self.cfg.cpu_threshold_permille, streak),
                    });
                }
                if *working_set_kb >= self.cfg.memory_threshold_kb
                    && self.cooled_down(*key, IncidentKind::MemoryHigh, ts_utc_ms)
                {
                    return Some(Finding {
                        kind: IncidentKind::MemoryHigh,
                        severity: Severity::Warning,
                        exe_name: Some(exe),
                        summary: format!("memory_high|{}", self.cfg.memory_threshold_kb / 1024),
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
        assert_eq!(f.summary, "app_crash|3221225477", "0xC0000005 as a decimal parameter");
        assert!(!f.summary.contains("synth"));
    }

    #[test]
    fn summaries_are_language_neutral_codes() {
        let mut d = Detector::new(cfg());
        d.observe(&started(1), 0);
        d.observe(&m(1, 950, 1), 1);
        d.observe(&m(1, 950, 1), 2);
        let cpu = d.observe(&m(1, 950, 1), 3).unwrap();
        assert_eq!(cpu.summary, "cpu_sustained|900|3");
        let mem = d.observe(&m(1, 0, 5_000_000), 4).unwrap();
        assert_eq!(mem.summary, "memory_high|3906");
        let hang = d.observe(&EventKind::AppHang { exe_name: ExeName::new("synth.exe").unwrap() }, 5).unwrap();
        assert_eq!(hang.summary, "app_hang");
        for s in [&cpu.summary, &mem.summary, &hang.summary] {
            assert!(s.is_ascii() && s.split('|').next().unwrap().chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
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

    // ---- saúde da máquina ----

    fn health(category: HealthCategory, event_id: u16, code: Option<u32>) -> EventKind {
        EventKind::HealthEvent { category, event_id, code }
    }

    #[test]
    fn an_unexpected_shutdown_without_a_stop_code_opens_a_critical_incident_with_no_app() {
        let mut d = Detector::new(IncidentConfig::default());
        let f = d.observe(&health(HealthCategory::UnexpectedShutdown, 6008, None), 10).unwrap();
        assert_eq!((f.kind, f.severity, f.exe_name, f.summary.as_str()), (IncidentKind::UnexpectedShutdown, Severity::Critical, None, "unexpected_shutdown|6008"));
    }

    #[test]
    fn kernel_power_41_with_a_stop_code_is_a_blue_screen_and_without_one_a_power_loss() {
        let mut d = Detector::new(IncidentConfig::default());
        let f = d.observe(&health(HealthCategory::UnexpectedShutdown, 41, Some(209)), 10).unwrap();
        assert_eq!((f.kind, f.summary.as_str()), (IncidentKind::BlueScreen, "blue_screen|209"));
        let mut d = Detector::new(IncidentConfig::default());
        let f = d.observe(&health(HealthCategory::UnexpectedShutdown, 41, Some(0)), 10).unwrap();
        assert_eq!(f.kind, IncidentKind::UnexpectedShutdown, "a zero stop code is a power loss, not a blue screen");
    }

    #[test]
    fn the_events_of_one_bad_shutdown_open_a_single_incident() {
        let mut d = Detector::new(IncidentConfig::default());
        assert!(d.observe(&health(HealthCategory::UnexpectedShutdown, 41, Some(209)), 10).is_some());
        assert!(d.observe(&health(HealthCategory::BugCheck, 1001, Some(209)), 4_000).is_none(), "same shutdown");
        assert!(d.observe(&health(HealthCategory::UnexpectedShutdown, 6008, None), 5_000).is_none(), "same shutdown");
        assert!(d.observe(&health(HealthCategory::BugCheck, 1001, Some(10)), 10 + 300_000).is_some(), "a later one is a new incident");
    }

    #[test]
    fn a_whea_error_opens_a_hardware_incident_and_repeats_are_condensed() {
        let mut d = Detector::new(IncidentConfig::default());
        let f = d.observe(&health(HealthCategory::HardwareError, 18, None), 10).unwrap();
        assert_eq!((f.kind, f.severity, f.summary.as_str()), (IncidentKind::HardwareError, Severity::Critical, "hardware_error|18"));
        assert!(d.observe(&health(HealthCategory::HardwareError, 17, None), 20_000).is_none());
    }

    #[test]
    fn other_health_events_do_not_open_incidents() {
        let mut d = Detector::new(IncidentConfig::default());
        for c in [
            HealthCategory::DisplayDriverReset,
            HealthCategory::DiskError,
            HealthCategory::FileSystemError,
            HealthCategory::ServiceCrash,
            HealthCategory::UpdateFailure,
            HealthCategory::SleepEntered,
            HealthCategory::Resumed,
        ] {
            assert!(d.observe(&health(c, 1, None), 10).is_none(), "{c:?}");
        }
    }

    #[test]
    fn throttling_opens_one_warning_per_half_hour() {
        let mut d = Detector::new(IncidentConfig::default());
        let f = d.throttling_finding(Some(70), Some(95), 1_000).unwrap();
        assert_eq!((f.kind, f.severity, f.exe_name, f.summary.as_str()), (IncidentKind::Throttling, Severity::Warning, None, "throttling|70|95"));
        assert!(d.throttling_finding(Some(70), Some(95), 1_000 + 29 * 60_000).is_none());
        assert!(d.throttling_finding(Some(70), Some(95), 1_000 + 30 * 60_000).is_some());
    }
}
