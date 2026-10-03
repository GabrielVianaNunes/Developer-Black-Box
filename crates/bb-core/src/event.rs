//! Modelo de eventos: allowlist por schema, sem campos de texto livre.

use crate::state::{ReasonCode, RecorderState};
use serde::Serialize;

/// Apenas o nome do arquivo do executável, normalizado em minúsculas.
/// Nunca caminho, linha de comando ou título de janela.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct ExeName(String);

#[derive(Debug, PartialEq, Eq)]
pub enum ExeNameError {
    Empty,
    TooLong,
    HasPathSeparator,
    InvalidChar,
}

impl ExeName {
    pub const MAX_LEN: usize = 64;

    pub fn new(raw: &str) -> Result<Self, ExeNameError> {
        if raw.is_empty() {
            return Err(ExeNameError::Empty);
        }
        if raw.contains(['/', '\\', ':']) {
            return Err(ExeNameError::HasPathSeparator);
        }
        if raw.chars().count() > Self::MAX_LEN {
            return Err(ExeNameError::TooLong);
        }
        if raw.chars().any(|c| c.is_control()) {
            return Err(ExeNameError::InvalidChar);
        }
        Ok(ExeName(raw.to_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Instância de processo: o Windows reutiliza PIDs, então o horário de início
/// faz parte da identidade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct ProcessKey {
    pub pid: u32,
    pub start_time_ms: i64,
}

/// Categoria de um evento de saúde da máquina. Enumeração FECHADA: o que o Windows escreveu na mensagem
/// nunca chega aqui, só em qual destas famílias o evento se encaixa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum HealthCategory {
    /// Desligamento inesperado (Kernel-Power 41, EventLog 6008).
    UnexpectedShutdown,
    /// Tela azul (código de verificação numérico).
    BugCheck,
    /// Erro de hardware reportado pelo WHEA.
    HardwareError,
    /// Reinício do driver de vídeo (Display 4101).
    DisplayDriverReset,
    /// Erro do driver de disco.
    DiskError,
    /// Erro do sistema de arquivos NTFS.
    FileSystemError,
    /// Serviço do Windows que encerrou sem querer (só a contagem; nunca o nome do serviço).
    ServiceCrash,
    /// Falha na instalação de uma atualização do Windows (só o código de erro).
    UpdateFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum EventKind {
    ProcessStarted { key: ProcessKey, exe_name: ExeName, parent_pid: u32 },
    ProcessExited { key: ProcessKey, exit_code: Option<i32> },
    ProcessMetrics { key: ProcessKey, cpu_permille: u16, working_set_kb: u64 },
    SystemMetrics { cpu_permille: u16, mem_used_kb: u64, mem_total_kb: u64 },
    /// Falha de aplicativo registrada pelo Windows (Event Log, ID 1000). Só o nome do
    /// executável e o código de exceção numérico; nunca o texto da mensagem.
    AppCrash { exe_name: ExeName, exception_code: u32 },
    /// Aplicativo que deixou de responder (Event Log, ID 1002).
    AppHang { exe_name: ExeName },
    RecorderStateChanged { state: RecorderState, reason: ReasonCode },
    /// Marcador pré-definido pelo usuário; sem texto.
    UserMarker { code: u16 },
    /// Evento de saúde da máquina vindo do Event Log `System`. Só a categoria, o ID do evento (de uma lista fixa) e
    /// um número opcional (ex. o código da tela azul); nunca o texto da mensagem, nomes de serviço, caminhos ou usuários.
    /// Só `PrivacyGuard::admit_health` cria este evento, porque ele não depende do app em primeiro plano.
    HealthEvent { category: HealthCategory, event_id: u16, code: Option<u32> },
}

/// Só o Privacy Guard constrói este tipo (construtor `pub(crate)`), então o
/// recorder não consegue receber um evento que não passou pelo Guard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValidatedEvent {
    seq: u64,
    ts_utc_ms: i64,
    kind: EventKind,
}

impl ValidatedEvent {
    pub(crate) fn new(seq: u64, ts_utc_ms: i64, kind: EventKind) -> Self {
        Self { seq, ts_utc_ms, kind }
    }
    pub fn seq(&self) -> u64 {
        self.seq
    }
    pub fn ts_utc_ms(&self) -> i64 {
        self.ts_utc_ms
    }
    pub fn kind(&self) -> &EventKind {
        &self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_is_normalized() {
        assert_eq!(ExeName::new("Notepad.EXE").unwrap().as_str(), "notepad.exe");
    }

    #[test]
    fn exe_name_rejects_paths() {
        assert_eq!(ExeName::new("C:\\Users\\x\\a.exe"), Err(ExeNameError::HasPathSeparator));
        assert_eq!(ExeName::new("dir/a.exe"), Err(ExeNameError::HasPathSeparator));
    }

    #[test]
    fn exe_name_rejects_empty_long_and_control() {
        assert_eq!(ExeName::new(""), Err(ExeNameError::Empty));
        assert_eq!(ExeName::new(&"a".repeat(65)), Err(ExeNameError::TooLong));
        assert_eq!(ExeName::new("a\nb.exe"), Err(ExeNameError::InvalidChar));
    }
}
