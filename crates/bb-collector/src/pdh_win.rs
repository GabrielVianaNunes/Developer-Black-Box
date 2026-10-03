//! Contadores de desempenho do Windows (PDH), sem administrador e com nomes em INGLÊS.
//!
//! `PdhAddEnglishCounterW` aceita os caminhos em inglês em qualquer idioma do Windows (no Windows em português os nomes
//! comuns são traduzidos). Lê só contadores do SISTEMA. Os nomes de instância (zona térmica, adaptador de rede, motor de
//! GPU com PID) são usados só para agregar e descartados em `telemetry` (puro, onde estão a validação e os testes).
//! Contador ausente ou valor inválido vira `None` ("indisponível"), nunca erro.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_HCOUNTER, PDH_HQUERY, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
};

use crate::sample::CollectError;
use crate::telemetry::{RawCounters, SampleBuilder, TelemetrySource};
use bb_core::HealthSample;

/// `PDH_CSTATUS_VALID_DATA` e `PDH_CSTATUS_NEW_DATA`: os dois significam valor utilizável.
fn usable(status: u32) -> bool {
    status == 0 || status == 1
}

struct Counter(PDH_HCOUNTER);

pub struct WindowsTelemetrySource {
    query: PDH_HQUERY,
    temperature: Option<Counter>,
    passive_limit: Option<Counter>,
    cpu_load: Option<Counter>,
    cpu_perf: Option<Counter>,
    cpu_freq: Option<Counter>,
    mem_commit: Option<Counter>,
    mem_available: Option<Counter>,
    page_faults: Option<Counter>,
    disk_latency: Option<Counter>,
    disk_busy: Option<Counter>,
    net_rx_errors: Option<Counter>,
    net_tx_errors: Option<Counter>,
    gpu: Option<Counter>,
    builder: SampleBuilder,
}

impl WindowsTelemetrySource {
    /// Abre a consulta e prepara os contadores. Os que não existirem nesta máquina ficam `None`. Se nem a consulta abrir,
    /// o erro faz a fonte ficar indisponível.
    pub fn new() -> Result<Self, CollectError> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` é um destino válido; a consulta é fechada no `Drop`.
        let status = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) };
        if status != 0 {
            return Err(CollectError(format!("PdhOpenQuery: {status:#x}")));
        }
        let add = |path: &str| -> Option<Counter> {
            let mut h = PDH_HCOUNTER::default();
            let path = HSTRING::from(path);
            // SAFETY: `path` vive durante a chamada; `h` recebe um handle válido se houver êxito.
            let status = unsafe { PdhAddEnglishCounterW(query, &path, 0, &mut h) };
            (status == 0).then_some(Counter(h))
        };
        let me = Self {
            query,
            temperature: add("\\Thermal Zone Information(*)\\Temperature"),
            passive_limit: add("\\Thermal Zone Information(*)\\% Passive Limit"),
            cpu_load: add("\\Processor Information(_Total)\\% Processor Time"),
            cpu_perf: add("\\Processor Information(_Total)\\% Processor Performance"),
            cpu_freq: add("\\Processor Information(_Total)\\Processor Frequency"),
            mem_commit: add("\\Memory\\% Committed Bytes In Use"),
            mem_available: add("\\Memory\\Available MBytes"),
            page_faults: add("\\Memory\\Page Faults/sec"),
            disk_latency: add("\\PhysicalDisk(_Total)\\Avg. Disk sec/Transfer"),
            disk_busy: add("\\PhysicalDisk(_Total)\\% Disk Time"),
            net_rx_errors: add("\\Network Interface(*)\\Packets Received Errors"),
            net_tx_errors: add("\\Network Interface(*)\\Packets Outbound Errors"),
            gpu: add("\\GPU Engine(*)\\Utilization Percentage"),
            builder: SampleBuilder::new(),
        };
        // Contadores de taxa precisam de duas coletas: esta primeira só prepara a referência.
        // SAFETY: consulta válida.
        unsafe { PdhCollectQueryData(query) };
        Ok(me)
    }
}

// SAFETY: os handles do PDH são ponteiros opacos que o PDH aceita de qualquer thread desde que não sejam usados ao
// mesmo tempo; a fonte pertence ao motor, que só a usa dentro do mutex do aplicativo (uma thread por vez).
unsafe impl Send for WindowsTelemetrySource {}

impl Drop for WindowsTelemetrySource {
    fn drop(&mut self) {
        // SAFETY: consulta aberta por PdhOpenQueryW e fechada uma única vez.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

fn single(c: &Option<Counter>) -> Option<f64> {
    let c = c.as_ref()?;
    let mut v = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: `v` é um destino válido; o handle pertence a uma consulta aberta.
    let status = unsafe { PdhGetFormattedCounterValue(c.0, PDH_FMT_DOUBLE, None, &mut v) };
    // SAFETY: com `PDH_FMT_DOUBLE` o campo ativo da união é `doubleValue`.
    (status == 0 && usable(v.CStatus)).then(|| unsafe { v.Anonymous.doubleValue })
}

/// Valores de um contador com instâncias curinga: (nome da instância, valor), só os utilizáveis.
fn array(c: &Option<Counter>) -> Vec<(String, f64)> {
    let Some(c) = c.as_ref() else { return Vec::new() };
    let (mut bytes, mut count) = (0u32, 0u32);
    // SAFETY: primeira chamada só descobre o tamanho do buffer.
    let status = unsafe { PdhGetFormattedCounterArrayW(c.0, PDH_FMT_DOUBLE, &mut bytes, &mut count, None) };
    if status != PDH_MORE_DATA || bytes == 0 || bytes > 8 * 1024 * 1024 {
        return Vec::new();
    }
    // u64 garante o alinhamento da estrutura; os nomes ficam dentro deste mesmo buffer.
    let mut buf = vec![0u64; (bytes as usize).div_ceil(8)];
    let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    // SAFETY: o buffer tem pelo menos `bytes` bytes; `count` itens são escritos nele.
    let status = unsafe { PdhGetFormattedCounterArrayW(c.0, PDH_FMT_DOUBLE, &mut bytes, &mut count, Some(items)) };
    if status != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..count as usize {
        // SAFETY: `i < count`, e cada nome aponta para texto terminado em NUL dentro do buffer.
        let (name, value) = unsafe {
            let item = &*items.add(i);
            let name = if item.szName.is_null() { String::new() } else { item.szName.to_string().unwrap_or_default() };
            (name, item.FmtValue)
        };
        if usable(value.CStatus) {
            // SAFETY: campo ativo `doubleValue` por causa do formato pedido.
            out.push((name, unsafe { value.Anonymous.doubleValue }));
        }
    }
    out
}

fn sum(values: &[(String, f64)]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().map(|(_, v)| *v).sum())
}

impl TelemetrySource for WindowsTelemetrySource {
    fn sample(&mut self) -> Result<HealthSample, CollectError> {
        // SAFETY: consulta válida.
        let status = unsafe { PdhCollectQueryData(self.query) };
        if status != 0 {
            return Err(CollectError(format!("PdhCollectQueryData: {status:#x}")));
        }
        let rx = array(&self.net_rx_errors);
        let tx = array(&self.net_tx_errors);
        let net_total = match (sum(&rx), sum(&tx)) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        let raw = RawCounters {
            temperatures_k: array(&self.temperature).into_iter().map(|(_, v)| v).collect(),
            passive_limits_pct: array(&self.passive_limit).into_iter().map(|(_, v)| v).collect(),
            cpu_load_pct: single(&self.cpu_load),
            cpu_perf_pct: single(&self.cpu_perf),
            cpu_freq_mhz: single(&self.cpu_freq),
            mem_commit_pct: single(&self.mem_commit),
            mem_available_mb: single(&self.mem_available),
            page_faults_per_sec: single(&self.page_faults),
            disk_latency_sec: single(&self.disk_latency),
            disk_busy_pct: single(&self.disk_busy),
            net_errors_total: net_total,
            gpu_engines: array(&self.gpu),
        };
        Ok(self.builder.build(&raw))
    }
}
