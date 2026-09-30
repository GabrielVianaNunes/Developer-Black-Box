//! Amostras cruas do sistema. Só campos numéricos e o nome do executável.

use bb_core::{ExeName, Observation, ProcessKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSample {
    pub key: ProcessKey,
    pub exe_name: ExeName,
    pub parent_pid: u32,
    /// Tempo de CPU acumulado (kernel + usuário), em unidades de 100 ns.
    pub cpu_time_100ns: u64,
    pub working_set_kb: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemSample {
    pub cpu_permille: u16,
    pub mem_used_kb: u64,
    pub mem_total_kb: u64,
}

#[derive(Debug)]
pub struct CollectError(pub String);

impl std::fmt::Display for CollectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "collect error: {}", self.0)
    }
}
impl std::error::Error for CollectError {}

/// Fonte de dados de atividade. Só é consultada enquanto o estado é `Recording`.
pub trait ProcessSource {
    fn processes(&mut self) -> Result<Vec<ProcessSample>, CollectError>;
    fn system(&mut self) -> Result<SystemSample, CollectError>;
}

/// Fonte de sinais de contexto. Consultada sempre, inclusive durante a pausa
/// manual, porque o Guard precisa avaliar o contexto continuamente.
pub trait ContextSource {
    fn observe(&mut self) -> Observation;
}
