//! Máquina de estados de gravação: função pura, sem relógio nem I/O.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SensitiveReason {
    ProtectedApp,
    SessionLocked,
}

/// `Unknown` conta como bloqueado: ausência de sinal nunca prova contexto seguro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacyContext {
    Safe,
    /// O primeiro plano é um app protegido com autorização de teste ativa: grava, mas só
    /// eventos técnicos desse executável (o Guard filtra no `admit`).
    AuthorizedOnly,
    Sensitive(SensitiveReason),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecorderInputs {
    pub starting: bool,
    pub shutting_down: bool,
    pub manual_paused: bool,
    pub privacy: PrivacyContext,
    pub guard_healthy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum RecorderState {
    Starting,
    ShuttingDown,
    ManualPause,
    PrivacyBlocked,
    SafetyFault,
    Recording,
}

/// Enum fechado: nunca carrega texto livre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ReasonCode {
    Shutdown,
    ManualPause,
    Starting,
    SensitiveApp,
    SessionLocked,
    ContextUnknown,
    DetectorUnavailable,
    /// Falha ao coletar ou gravar (ex. disco cheio): o ícone não pode continuar verde.
    RecorderFault,
    /// Gravando em modo de teste: só o aplicativo autorizado, nada dos demais.
    RestrictedTest,
    None,
}

impl RecorderState {
    /// Somente `Recording` permite persistir eventos (e acende a luz verde).
    pub fn is_recording(self) -> bool {
        self == RecorderState::Recording
    }
}

/// Prioridade: ShuttingDown > ManualPause > Starting > PrivacyBlocked (contexto
/// sensível definido) > SafetyFault (guard indisponível) > PrivacyBlocked
/// (contexto desconhecido) > Recording.
///
/// Um contexto sensível definido tem precedência sobre a falha do guard; um
/// contexto apenas desconhecido não esconde a falha do guard.
pub fn derive(i: RecorderInputs) -> (RecorderState, ReasonCode) {
    use RecorderState::*;
    if i.shutting_down {
        return (ShuttingDown, ReasonCode::Shutdown);
    }
    if i.manual_paused {
        return (ManualPause, ReasonCode::ManualPause);
    }
    if i.starting {
        return (Starting, ReasonCode::Starting);
    }
    if let PrivacyContext::Sensitive(r) = i.privacy {
        let code = match r {
            SensitiveReason::ProtectedApp => ReasonCode::SensitiveApp,
            SensitiveReason::SessionLocked => ReasonCode::SessionLocked,
        };
        return (PrivacyBlocked, code);
    }
    if !i.guard_healthy {
        return (SafetyFault, ReasonCode::DetectorUnavailable);
    }
    match i.privacy {
        PrivacyContext::Safe => (Recording, ReasonCode::None),
        PrivacyContext::AuthorizedOnly => (Recording, ReasonCode::RestrictedTest),
        _ => (PrivacyBlocked, ReasonCode::ContextUnknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RecorderInputs {
        RecorderInputs {
            starting: false,
            shutting_down: false,
            manual_paused: false,
            privacy: PrivacyContext::Safe,
            guard_healthy: true,
        }
    }

    #[test]
    fn records_only_when_everything_allows() {
        assert_eq!(derive(base()).0, RecorderState::Recording);
    }

    #[test]
    fn shutdown_beats_everything() {
        let i = RecorderInputs { shutting_down: true, manual_paused: true, ..base() };
        assert_eq!(derive(i).0, RecorderState::ShuttingDown);
    }

    #[test]
    fn manual_pause_beats_privacy_and_fault() {
        let i = RecorderInputs {
            manual_paused: true,
            privacy: PrivacyContext::Sensitive(SensitiveReason::ProtectedApp),
            guard_healthy: false,
            ..base()
        };
        assert_eq!(derive(i).0, RecorderState::ManualPause);
    }

    #[test]
    fn unknown_context_never_records() {
        let i = RecorderInputs { privacy: PrivacyContext::Unknown, ..base() };
        assert_eq!(derive(i), (RecorderState::PrivacyBlocked, ReasonCode::ContextUnknown));
    }

    #[test]
    fn unhealthy_guard_is_a_fault_even_if_context_looks_safe() {
        let i = RecorderInputs { guard_healthy: false, ..base() };
        assert_eq!(derive(i).0, RecorderState::SafetyFault);
    }

    #[test]
    fn unhealthy_guard_with_unknown_context_reports_fault() {
        let i = RecorderInputs { guard_healthy: false, privacy: PrivacyContext::Unknown, ..base() };
        assert_eq!(derive(i).0, RecorderState::SafetyFault);
    }

    #[test]
    fn sensitive_context_reports_privacy_block() {
        let i = RecorderInputs {
            privacy: PrivacyContext::Sensitive(SensitiveReason::SessionLocked),
            guard_healthy: false,
            ..base()
        };
        assert_eq!(derive(i), (RecorderState::PrivacyBlocked, ReasonCode::SessionLocked));
    }

    #[test]
    fn starting_never_records() {
        let i = RecorderInputs { starting: true, ..base() };
        assert_eq!(derive(i).0, RecorderState::Starting);
    }
}
