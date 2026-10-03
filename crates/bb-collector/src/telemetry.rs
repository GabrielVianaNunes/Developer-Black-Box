//! Telemetria de desempenho do sistema (contadores PDH): validação e agregação, sem Windows.
//!
//! O que sai daqui são NÚMEROS agregados do sistema (`HealthSample`). Os nomes de instância dos contadores (zona
//! térmica, adaptador de rede, motor de GPU com PID) são usados só para agregar e descartados na hora: nada por
//! processo, por usuário ou por conexão. Cada valor é validado (faixa e unidade); inválido vira `None` ("indisponível").

use bb_core::HealthSample;

use crate::sample::CollectError;

/// Fonte da telemetria. `Err` = fonte indisponível (o motor segue sem ela, sem alarme).
pub trait TelemetrySource {
    fn sample(&mut self) -> Result<HealthSample, CollectError>;
}

/// Valores crus lidos dos contadores (já em `f64`), antes da validação. As listas guardam um valor por instância.
#[derive(Clone, Debug, Default)]
pub struct RawCounters {
    /// `\Thermal Zone Information(*)\Temperature`, em kelvin.
    pub temperatures_k: Vec<f64>,
    /// `\Thermal Zone Information(*)\% Passive Limit`.
    pub passive_limits_pct: Vec<f64>,
    /// `\Processor Information(_Total)\% Processor Time`.
    pub cpu_load_pct: Option<f64>,
    /// `\Processor Information(_Total)\% Processor Performance`.
    pub cpu_perf_pct: Option<f64>,
    /// `\Processor Information(_Total)\Processor Frequency`, em MHz.
    pub cpu_freq_mhz: Option<f64>,
    /// `\Memory\% Committed Bytes In Use`.
    pub mem_commit_pct: Option<f64>,
    /// `\Memory\Available MBytes`.
    pub mem_available_mb: Option<f64>,
    /// `\Memory\Page Faults/sec`.
    pub page_faults_per_sec: Option<f64>,
    /// `\PhysicalDisk(_Total)\Avg. Disk sec/Transfer`, em segundos.
    pub disk_latency_sec: Option<f64>,
    /// `\PhysicalDisk(_Total)\% Disk Time`.
    pub disk_busy_pct: Option<f64>,
    /// Soma, entre os adaptadores, de `Packets Received Errors` + `Packets Outbound Errors` (contadores acumulados).
    pub net_errors_total: Option<f64>,
    /// `\GPU Engine(*)\Utilization Percentage`: (nome da instância, valor). O nome só serve para achar o motor 3D.
    pub gpu_engines: Vec<(String, f64)>,
}

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

/// Porcentagem de 0 a 100. Um pouco acima de 100 (arredondamento do contador) é limitado; muito acima é inválido.
pub fn pct(v: f64) -> Option<u8> {
    let v = finite(v)?;
    (-0.5..=105.0).contains(&v).then(|| v.clamp(0.0, 100.0).round() as u8)
}

/// Temperatura em kelvin: só faixa plausível de um computador (-73 °C a 177 °C). Zero e valores em décimos de kelvin
/// (como 3050) são rejeitados, não "corrigidos".
pub fn kelvin(v: f64) -> Option<u16> {
    let v = finite(v)?;
    (200.0..=450.0).contains(&v).then(|| v.round() as u16)
}

/// Desempenho relativo da CPU (pode passar de 100 com turbo): 0 a 1000 %.
pub fn perf_pct(v: f64) -> Option<u16> {
    let v = finite(v)?;
    (0.0..=1000.0).contains(&v).then(|| v.round() as u16)
}

/// Frequência em MHz: 1 a 20000.
pub fn mhz(v: f64) -> Option<u16> {
    let v = finite(v)?;
    (1.0..=20_000.0).contains(&v).then(|| v.round() as u16)
}

/// Contagem não negativa que cabe em 32 bits.
pub fn count32(v: f64) -> Option<u32> {
    let v = finite(v)?;
    (0.0..=f64::from(u32::MAX)).contains(&v).then(|| v.round() as u32)
}

/// Latência em segundos -> microssegundos. Aceita de 0 a 60 s; fora disso (ou negativa) é inválida.
pub fn latency_us(sec: f64) -> Option<u32> {
    let v = finite(sec)?;
    (0.0..=60.0).contains(&v).then(|| (v * 1_000_000.0).round() as u32)
}

/// Uso do motor 3D da GPU: soma dos motores `engtype_3D` (uma linha por processo e motor), limitada a 100. Sem nenhum
/// motor 3D (sem GPU ou contador ausente) é `None`. O texto da instância (que contém PID) é usado só para filtrar.
pub fn gpu_3d_pct(engines: &[(String, f64)]) -> Option<u8> {
    let mut any = false;
    let mut sum = 0.0;
    for (name, v) in engines {
        if name.contains("engtype_3D") {
            any = true;
            sum += finite(*v).filter(|v| *v >= 0.0)?;
        }
    }
    any.then(|| sum.min(100.0).round() as u8)
}

/// Monta amostras a partir dos valores crus. Guarda só o acumulado anterior dos erros de rede para calcular o delta.
#[derive(Debug, Default)]
pub struct SampleBuilder {
    net_prev: Option<f64>,
}

impl SampleBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Erros de rede desde a amostra anterior. A primeira leitura só vira referência; um acumulado menor que o anterior
    /// (adaptador removido, reinício do contador) não gera número negativo nem enorme: vira `None` e é a nova referência.
    fn net_delta(&mut self, total: Option<f64>) -> Option<u32> {
        let total = total.and_then(finite).filter(|t| *t >= 0.0);
        let prev = std::mem::replace(&mut self.net_prev, total);
        match (prev, total) {
            (Some(p), Some(t)) if t >= p => count32(t - p),
            _ => None,
        }
    }

    pub fn build(&mut self, raw: &RawCounters) -> HealthSample {
        let max_temp = raw.temperatures_k.iter().copied().filter_map(kelvin).max();
        let min_limit = raw.passive_limits_pct.iter().copied().filter_map(pct).min();
        HealthSample {
            thermal_kelvin: max_temp,
            passive_limit_pct: min_limit,
            cpu_load_pct: raw.cpu_load_pct.and_then(pct),
            cpu_perf_pct: raw.cpu_perf_pct.and_then(perf_pct),
            cpu_freq_mhz: raw.cpu_freq_mhz.and_then(mhz),
            mem_commit_pct: raw.mem_commit_pct.and_then(pct),
            mem_available_mb: raw.mem_available_mb.and_then(count32),
            page_faults_per_sec: raw.page_faults_per_sec.and_then(count32),
            disk_latency_us: raw.disk_latency_sec.and_then(latency_us),
            disk_busy_pct: raw.disk_busy_pct.and_then(pct),
            net_errors: self.net_delta(raw.net_errors_total),
            gpu_pct: gpu_3d_pct(&raw.gpu_engines),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_are_validated_and_clamped_only_slightly() {
        assert_eq!(pct(0.0), Some(0));
        assert_eq!(pct(57.4), Some(57));
        assert_eq!(pct(100.0), Some(100));
        assert_eq!(pct(100.4), Some(100), "counter rounding");
        assert_eq!(pct(130.0), None, "far above 100 is invalid, not clamped");
        assert_eq!(pct(-3.0), None);
        assert_eq!(pct(f64::NAN), None);
        assert_eq!(pct(f64::INFINITY), None);
    }

    #[test]
    fn kelvin_is_validated_by_unit() {
        assert_eq!(kelvin(305.0), Some(305));
        assert_eq!(kelvin(318.6), Some(319));
        assert_eq!(kelvin(0.0), None, "zero means no sensor");
        assert_eq!(kelvin(3050.0), None, "tenths of a kelvin are rejected, not converted");
        assert_eq!(kelvin(45.0), None, "a Celsius value is rejected");
        assert_eq!(kelvin(f64::NAN), None);
    }

    #[test]
    fn performance_frequency_and_counts_have_ranges() {
        assert_eq!(perf_pct(142.0), Some(142));
        assert_eq!(perf_pct(5000.0), None);
        assert_eq!(mhz(3_600.0), Some(3600));
        assert_eq!(mhz(0.0), None);
        assert_eq!(mhz(1e9), None);
        assert_eq!(count32(1234.6), Some(1235));
        assert_eq!(count32(-1.0), None);
        assert_eq!(count32(1e12), None);
    }

    #[test]
    fn disk_latency_converts_seconds_to_microseconds_and_rejects_nonsense() {
        assert_eq!(latency_us(0.0021), Some(2100));
        assert_eq!(latency_us(0.0), Some(0));
        assert_eq!(latency_us(60.0), Some(60_000_000));
        assert_eq!(latency_us(61.0), None);
        assert_eq!(latency_us(-0.1), None);
        assert_eq!(latency_us(f64::NAN), None);
    }

    fn eng(name: &str, v: f64) -> (String, f64) {
        (name.to_owned(), v)
    }

    #[test]
    fn gpu_sums_only_3d_engines_and_never_keeps_the_instance_text() {
        let engines = vec![
            eng("pid_4242_luid_0x00000000_0x0000A1B2_phys_0_eng_0_engtype_3D", 12.5),
            eng("pid_777_luid_0x00000000_0x0000A1B2_phys_0_eng_0_engtype_3D", 30.0),
            eng("pid_777_luid_0x00000000_0x0000A1B2_phys_0_eng_3_engtype_VideoDecode", 80.0),
        ];
        assert_eq!(gpu_3d_pct(&engines), Some(43), "12.5 + 30 = 42.5 -> 43; the decode engine is ignored");
        let busy = vec![eng("a_engtype_3D", 70.0), eng("b_engtype_3D", 70.0)];
        assert_eq!(gpu_3d_pct(&busy), Some(100), "the sum is capped at 100");
    }

    #[test]
    fn gpu_without_3d_engines_or_with_bad_values_is_unavailable() {
        assert_eq!(gpu_3d_pct(&[]), None);
        assert_eq!(gpu_3d_pct(&[eng("x_engtype_Copy", 5.0)]), None);
        assert_eq!(gpu_3d_pct(&[eng("x_engtype_3D", f64::NAN)]), None);
        assert_eq!(gpu_3d_pct(&[eng("x_engtype_3D", -5.0)]), None);
    }

    #[test]
    fn thermal_takes_the_hottest_zone_and_the_lowest_passive_limit() {
        let raw = RawCounters {
            temperatures_k: vec![300.0, 331.0, 0.0, 3100.0],
            passive_limits_pct: vec![100.0, 80.0, 100.0],
            ..Default::default()
        };
        let s = SampleBuilder::new().build(&raw);
        assert_eq!(s.thermal_kelvin, Some(331), "invalid zones are ignored, the hottest valid one wins");
        assert_eq!(s.passive_limit_pct, Some(80));
    }

    #[test]
    fn a_zone_list_with_only_invalid_values_is_unavailable() {
        let raw = RawCounters { temperatures_k: vec![0.0, 9999.0], passive_limits_pct: vec![f64::NAN], ..Default::default() };
        let s = SampleBuilder::new().build(&raw);
        assert_eq!((s.thermal_kelvin, s.passive_limit_pct), (None, None));
        assert!(s.is_empty());
    }

    #[test]
    fn network_errors_are_a_delta_and_the_first_reading_is_only_a_reference() {
        let mut b = SampleBuilder::new();
        let raw = |t| RawCounters { net_errors_total: Some(t), ..Default::default() };
        assert_eq!(b.build(&raw(100.0)).net_errors, None);
        assert_eq!(b.build(&raw(100.0)).net_errors, Some(0));
        assert_eq!(b.build(&raw(107.0)).net_errors, Some(7));
        // um adaptador saiu: o acumulado caiu. Nada de número negativo/enorme; é a nova referência.
        assert_eq!(b.build(&raw(40.0)).net_errors, None);
        assert_eq!(b.build(&raw(43.0)).net_errors, Some(3));
        // o contador sumiu: None, e a referência é esquecida
        assert_eq!(b.build(&RawCounters::default()).net_errors, None);
        assert_eq!(b.build(&raw(50.0)).net_errors, None);
    }

    #[test]
    fn a_full_synthetic_reading_becomes_one_sample_of_plain_numbers() {
        let raw = RawCounters {
            temperatures_k: vec![318.0],
            passive_limits_pct: vec![100.0],
            cpu_load_pct: Some(23.4),
            cpu_perf_pct: Some(117.0),
            cpu_freq_mhz: Some(3_901.0),
            mem_commit_pct: Some(61.2),
            mem_available_mb: Some(10_240.0),
            page_faults_per_sec: Some(900.0),
            disk_latency_sec: Some(0.0004),
            disk_busy_pct: Some(3.0),
            net_errors_total: None,
            gpu_engines: vec![eng("pid_1_engtype_3D", 8.0)],
        };
        let s = SampleBuilder::new().build(&raw);
        assert_eq!(
            s,
            HealthSample {
                thermal_kelvin: Some(318),
                passive_limit_pct: Some(100),
                cpu_load_pct: Some(23),
                cpu_perf_pct: Some(117),
                cpu_freq_mhz: Some(3901),
                mem_commit_pct: Some(61),
                mem_available_mb: Some(10_240),
                page_faults_per_sec: Some(900),
                disk_latency_us: Some(400),
                disk_busy_pct: Some(3),
                net_errors: None,
                gpu_pct: Some(8),
            }
        );
    }

    #[test]
    fn nothing_read_is_an_empty_sample() {
        assert!(SampleBuilder::new().build(&RawCounters::default()).is_empty());
    }
}
