//! Converte fotografias sucessivas de processos em eventos de início, fim e métricas.
//!
//! Lógica pura: sem relógio e sem acesso ao Windows.

use std::collections::{HashMap, HashSet};

use bb_core::{EventKind, ProcessKey};

use crate::sample::ProcessSample;

#[derive(Clone, Debug)]
pub struct MetricsConfig {
    /// Emite métricas de processo a cada N ciclos (0 desliga).
    pub every_n_ticks: u32,
    pub min_cpu_permille: u16,
    pub min_working_set_kb: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self { every_n_ticks: 6, min_cpu_permille: 10, min_working_set_kb: 100_000 }
    }
}

struct Tracked {
    cpu_time_100ns: u64,
    /// Adotado sem evento de início (nasceu durante uma pausa/bloqueio):
    /// nenhum evento seu é emitido, nem métricas nem saída.
    silent: bool,
}

pub struct Differ {
    known: HashMap<ProcessKey, Tracked>,
    last_wall_ms: Option<u64>,
    ncpu: u64,
    metrics: MetricsConfig,
    tick_no: u64,
}

impl Differ {
    pub fn new(metrics: MetricsConfig, ncpu: usize) -> Self {
        Self { known: HashMap::new(), last_wall_ms: None, ncpu: ncpu.max(1) as u64, metrics, tick_no: 0 }
    }

    /// Esquece tudo. Chamado quando a gravação para: nada do que acontece
    /// enquanto isso é lembrado nem reconstruído depois.
    pub fn reset(&mut self) {
        self.known.clear();
        self.last_wall_ms = None;
    }

    /// `silent_from`: processos iniciados a partir deste instante (UTC ms) e ainda
    /// desconhecidos nasceram durante uma pausa; são adotados sem gerar eventos.
    pub fn tick(&mut self, samples: &[ProcessSample], wall_ms: u64, silent_from: Option<i64>) -> Vec<EventKind> {
        self.tick_no += 1;
        let dt_ms = self.last_wall_ms.map_or(0, |l| wall_ms.saturating_sub(l));
        self.last_wall_ms = Some(wall_ms);
        let n = u64::from(self.metrics.every_n_ticks);
        let metrics_due = n > 0 && self.tick_no % n == 0;

        let mut events = Vec::new();
        let mut seen: HashSet<ProcessKey> = HashSet::with_capacity(samples.len());
        for s in samples {
            seen.insert(s.key);
            match self.known.get_mut(&s.key) {
                None => {
                    let silent = silent_from.is_some_and(|t| s.key.start_time_ms >= t);
                    self.known.insert(s.key, Tracked { cpu_time_100ns: s.cpu_time_100ns, silent });
                    if !silent {
                        events.push(EventKind::ProcessStarted { key: s.key, exe_name: s.exe_name.clone(), parent_pid: s.parent_pid });
                    }
                }
                Some(t) => {
                    let prev = std::mem::replace(&mut t.cpu_time_100ns, s.cpu_time_100ns);
                    if t.silent || !metrics_due || dt_ms == 0 {
                        continue;
                    }
                    let delta = s.cpu_time_100ns.saturating_sub(prev);
                    let denom = dt_ms * 10_000 * self.ncpu;
                    let cpu = (delta.saturating_mul(1000) / denom).min(1000) as u16;
                    if cpu >= self.metrics.min_cpu_permille || s.working_set_kb >= self.metrics.min_working_set_kb {
                        events.push(EventKind::ProcessMetrics { key: s.key, cpu_permille: cpu, working_set_kb: s.working_set_kb });
                    }
                }
            }
        }

        let mut gone: Vec<ProcessKey> = self.known.keys().filter(|k| !seen.contains(*k)).copied().collect();
        gone.sort_by_key(|k| (k.pid, k.start_time_ms));
        for k in gone {
            if let Some(t) = self.known.remove(&k) {
                if !t.silent {
                    // A API de snapshot não informa o código de saída de quem já terminou.
                    events.push(EventKind::ProcessExited { key: k, exit_code: None });
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::ExeName;

    fn s(pid: u32, start: i64, cpu: u64, ws: u64) -> ProcessSample {
        ProcessSample {
            key: ProcessKey { pid, start_time_ms: start },
            exe_name: ExeName::new("synth.exe").unwrap(),
            parent_pid: 1,
            cpu_time_100ns: cpu,
            working_set_kb: ws,
        }
    }

    fn every_tick() -> MetricsConfig {
        MetricsConfig { every_n_ticks: 1, min_cpu_permille: 0, min_working_set_kb: 0 }
    }

    #[test]
    fn first_sight_is_a_start_and_disappearance_is_an_exit() {
        let mut d = Differ::new(every_tick(), 1);
        let e = d.tick(&[s(1, 100, 0, 5)], 1_000, None);
        assert!(matches!(e[..], [EventKind::ProcessStarted { .. }]));
        let e = d.tick(&[], 2_000, None);
        assert!(matches!(e[..], [EventKind::ProcessExited { .. }]));
    }

    #[test]
    fn pid_reuse_with_a_new_start_time_is_a_different_instance() {
        let mut d = Differ::new(every_tick(), 1);
        d.tick(&[s(7, 100, 0, 5)], 1_000, None);
        let e = d.tick(&[s(7, 500, 0, 5)], 2_000, None);
        assert!(e.iter().any(|x| matches!(x, EventKind::ProcessExited { key, .. } if key.start_time_ms == 100)));
        assert!(e.iter().any(|x| matches!(x, EventKind::ProcessStarted { key, .. } if key.start_time_ms == 500)));
    }

    #[test]
    fn cpu_permille_uses_delta_over_wall_time_and_core_count() {
        let mut d = Differ::new(every_tick(), 4);
        d.tick(&[s(1, 100, 0, 5)], 1_000, None);
        // 1 s de parede, 4 núcleos: 1 s de CPU = 250 permil do total.
        let e = d.tick(&[s(1, 100, 10_000_000, 5)], 2_000, None);
        assert!(matches!(e[..], [EventKind::ProcessMetrics { cpu_permille: 250, .. }]));
    }

    #[test]
    fn cpu_permille_is_capped_at_1000() {
        let mut d = Differ::new(every_tick(), 1);
        d.tick(&[s(1, 100, 0, 5)], 1_000, None);
        let e = d.tick(&[s(1, 100, 900_000_000, 5)], 2_000, None);
        assert!(matches!(e[..], [EventKind::ProcessMetrics { cpu_permille: 1000, .. }]));
    }

    #[test]
    fn metrics_respect_the_thresholds_and_period() {
        let cfg = MetricsConfig { every_n_ticks: 2, min_cpu_permille: 500, min_working_set_kb: 1_000 };
        let mut d = Differ::new(cfg, 1);
        d.tick(&[s(1, 1, 0, 10)], 1_000, None);
        // tick 2 é "devido", mas nem CPU nem memória passam do limite
        assert!(d.tick(&[s(1, 1, 0, 10)], 2_000, None).is_empty());
        // tick 3 não é devido
        assert!(d.tick(&[s(1, 1, 10_000_000, 9_999)], 3_000, None).is_empty());
        // tick 4: memória acima do limite
        assert_eq!(d.tick(&[s(1, 1, 10_000_000, 9_999)], 4_000, None).len(), 1);
    }

    #[test]
    fn processes_born_during_a_gap_are_adopted_silently() {
        let mut d = Differ::new(every_tick(), 1);
        // gap começou em 1_000; um processo iniciou em 1_500, outro já existia (start 200)
        let e = d.tick(&[s(1, 200, 0, 5), s(2, 1_500, 0, 5)], 5_000, Some(1_000));
        assert_eq!(e.len(), 1);
        assert!(matches!(e[0], EventKind::ProcessStarted { key, .. } if key.pid == 1));
        // nem métricas nem saída do processo silencioso aparecem depois
        let e = d.tick(&[s(1, 200, 0, 5), s(2, 1_500, 5_000_000, 5)], 6_000, None);
        assert!(e.iter().all(|x| !matches!(x, EventKind::ProcessMetrics { key, .. } if key.pid == 2)));
        let e = d.tick(&[s(1, 200, 0, 5)], 7_000, None);
        assert!(e.iter().all(|x| !matches!(x, EventKind::ProcessExited { key, .. } if key.pid == 2)));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut d = Differ::new(every_tick(), 1);
        d.tick(&[s(1, 100, 0, 5)], 1_000, None);
        d.reset();
        // sem lembrança: nenhuma saída é inferida para o que existia antes do reset
        assert!(d.tick(&[], 2_000, None).is_empty());
    }
}
