//! Identidade visual e textos da bandeja: tudo derivado do estado real do recorder.
//!
//! Lógica pura, sem janela nem Windows, para poder ser testada.

use bb_core::{ReasonCode, RecorderState};

pub mod icon;

/// Luz do ícone. Verde só com a gravação efetivamente ativa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    Green,
    Red,
    Gray,
}

impl Light {
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Light::Green => [0x22, 0xc5, 0x5e],
            Light::Red => [0xef, 0x44, 0x44],
            Light::Gray => [0x9c, 0xa3, 0xaf],
        }
    }
}

/// Verde somente em `Recording`; cinza ao iniciar/encerrar; vermelho em todo o resto.
pub fn light_for(state: RecorderState) -> Light {
    match state {
        RecorderState::Recording => Light::Green,
        RecorderState::Starting | RecorderState::ShuttingDown => Light::Gray,
        RecorderState::ManualPause | RecorderState::PrivacyBlocked | RecorderState::SafetyFault => Light::Red,
    }
}

/// Texto do estado e do motivo. Nunca contém nomes de aplicativos.
pub fn describe(state: RecorderState, reason: ReasonCode) -> String {
    match state {
        RecorderState::Recording if reason == ReasonCode::RestrictedTest => {
            "Gravando (modo de teste: só o aplicativo autorizado)".into()
        }
        RecorderState::Recording => "Gravando".into(),
        RecorderState::Starting => "Iniciando".into(),
        RecorderState::ShuttingDown => "Encerrando".into(),
        RecorderState::ManualPause => "Pausado manualmente".into(),
        RecorderState::PrivacyBlocked | RecorderState::SafetyFault => {
            let why = match reason {
                ReasonCode::SensitiveApp => "contexto sensível (privacidade)",
                ReasonCode::SessionLocked => "sessão bloqueada ou tela segura",
                ReasonCode::ContextUnknown => "contexto de privacidade indeterminado",
                ReasonCode::DetectorUnavailable => "detector de privacidade indisponível",
                ReasonCode::RecorderFault => "falha ao coletar ou gravar",
                _ => "regra de privacidade",
            };
            format!("Suspenso: {why}")
        }
    }
}

pub fn tooltip(state: RecorderState, reason: ReasonCode) -> String {
    format!("Developer Black Box: {}", describe(state, reason))
}

/// O que o menu de contexto da bandeja deve mostrar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuModel {
    pub status: String,
    pub can_pause: bool,
    pub can_resume: bool,
}

pub fn menu_model(state: RecorderState, reason: ReasonCode, manually_paused: bool) -> MenuModel {
    let alive = state != RecorderState::ShuttingDown;
    MenuModel {
        status: format!("Status: {}", describe(state, reason)),
        can_pause: alive && !manually_paused,
        can_resume: alive && manually_paused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [RecorderState; 6] = [
        RecorderState::Starting,
        RecorderState::ShuttingDown,
        RecorderState::ManualPause,
        RecorderState::PrivacyBlocked,
        RecorderState::SafetyFault,
        RecorderState::Recording,
    ];

    #[test]
    fn green_only_when_recording() {
        for s in ALL {
            assert_eq!(light_for(s) == Light::Green, s == RecorderState::Recording, "{s:?}");
        }
    }

    #[test]
    fn paused_blocked_and_faulted_are_red() {
        for s in [RecorderState::ManualPause, RecorderState::PrivacyBlocked, RecorderState::SafetyFault] {
            assert_eq!(light_for(s), Light::Red);
        }
    }

    #[test]
    fn starting_and_shutting_down_are_gray() {
        assert_eq!(light_for(RecorderState::Starting), Light::Gray);
        assert_eq!(light_for(RecorderState::ShuttingDown), Light::Gray);
    }

    #[test]
    fn every_blocked_reason_is_explained_in_text() {
        for r in [
            ReasonCode::SensitiveApp,
            ReasonCode::SessionLocked,
            ReasonCode::ContextUnknown,
            ReasonCode::DetectorUnavailable,
            ReasonCode::RecorderFault,
        ] {
            let t = describe(RecorderState::PrivacyBlocked, r);
            assert!(t.starts_with("Suspenso: ") && t.len() > 12, "{t}");
        }
        assert_eq!(describe(RecorderState::ManualPause, ReasonCode::ManualPause), "Pausado manualmente");
    }

    #[test]
    fn restricted_test_mode_is_green_but_says_so_in_text() {
        assert_eq!(light_for(RecorderState::Recording), Light::Green);
        let t = describe(RecorderState::Recording, ReasonCode::RestrictedTest);
        assert!(t.contains("modo de teste"), "{t}");
        assert_eq!(describe(RecorderState::Recording, ReasonCode::None), "Gravando");
    }

    #[test]
    fn menu_pause_and_resume_availability() {
        let rec = menu_model(RecorderState::Recording, ReasonCode::None, false);
        assert!(rec.can_pause && !rec.can_resume);
        let paused = menu_model(RecorderState::ManualPause, ReasonCode::ManualPause, true);
        assert!(!paused.can_pause && paused.can_resume);
        // Retomada pedida mas ainda bloqueada por privacidade: já não está em pausa manual.
        let blocked = menu_model(RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp, false);
        assert!(blocked.can_pause && !blocked.can_resume);
        let down = menu_model(RecorderState::ShuttingDown, ReasonCode::Shutdown, false);
        assert!(!down.can_pause && !down.can_resume);
    }

    #[test]
    fn status_line_shows_the_reason_for_a_pause() {
        let m = menu_model(RecorderState::ManualPause, ReasonCode::ManualPause, true);
        assert_eq!(m.status, "Status: Pausado manualmente");
    }
}
