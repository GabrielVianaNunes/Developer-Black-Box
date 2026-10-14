//! Identidade visual e textos da bandeja: tudo derivado do estado real do recorder.
//!
//! Lógica pura, sem janela nem Windows, para poder ser testada. Os textos existem em inglês e
//! em português do Brasil; o idioma é uma escolha do usuário (`Lang`).

use bb_core::{ReasonCode, RecorderState};

pub mod icon;

/// Idioma da interface (o do painel e o dos textos nativos da bandeja).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    PtBr,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::PtBr];

    /// Código estável, usado na configuração salva e na interface.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::PtBr => "pt-BR",
        }
    }

    /// Só aceita os códigos exatos que o app conhece.
    pub fn parse(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Escolhe o idioma a partir do nome da localidade do Windows (ex. "pt-BR", "pt-PT", "en-US").
    /// Qualquer variante do português usa o português do Brasil; o resto usa inglês.
    pub fn from_locale(locale: &str) -> Lang {
        let l = locale.trim().to_ascii_lowercase();
        if l == "pt" || l.starts_with("pt-") || l.starts_with("pt_") {
            Lang::PtBr
        } else {
            Lang::En
        }
    }
}

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
pub fn describe(lang: Lang, state: RecorderState, reason: ReasonCode) -> String {
    let pt = lang == Lang::PtBr;
    match state {
        RecorderState::Recording if reason == ReasonCode::RestrictedTest => {
            if pt { "Gravando (modo de teste: só o aplicativo autorizado)" } else { "Recording (test mode: authorized app only)" }.into()
        }
        RecorderState::Recording => if pt { "Gravando" } else { "Recording" }.into(),
        RecorderState::Starting => if pt { "Iniciando" } else { "Starting" }.into(),
        RecorderState::ShuttingDown => if pt { "Encerrando" } else { "Shutting down" }.into(),
        RecorderState::ManualPause => if pt { "Pausado manualmente" } else { "Paused manually" }.into(),
        RecorderState::PrivacyBlocked | RecorderState::SafetyFault => {
            let why = match (reason, pt) {
                (ReasonCode::SensitiveApp, true) => "contexto sensível (privacidade)",
                (ReasonCode::SensitiveApp, false) => "sensitive context (privacy)",
                (ReasonCode::SessionLocked, true) => "sessão bloqueada ou tela segura",
                (ReasonCode::SessionLocked, false) => "session locked or secure screen",
                (ReasonCode::ContextUnknown, true) => "contexto de privacidade indeterminado",
                (ReasonCode::ContextUnknown, false) => "privacy context undetermined",
                (ReasonCode::DetectorUnavailable, true) => "detector de privacidade indisponível",
                (ReasonCode::DetectorUnavailable, false) => "privacy detector unavailable",
                (ReasonCode::RecorderFault, true) => "falha ao coletar ou gravar",
                (ReasonCode::RecorderFault, false) => "failed to collect or record",
                (_, true) => "regra de privacidade",
                (_, false) => "privacy rule",
            };
            if pt {
                format!("Suspenso: {why}")
            } else {
                format!("Suspended: {why}")
            }
        }
    }
}

pub fn tooltip(lang: Lang, state: RecorderState, reason: ReasonCode) -> String {
    format!("Developer Black Box: {}", describe(lang, state, reason))
}

/// Texto fixo do aviso do sistema para um tipo de incidente (`IncidentKind::as_str()`). Só o TIPO: nunca nome de
/// programa, número, caminho ou qualquer dado coletado. Tipo desconhecido recebe o texto genérico.
pub fn incident_notice(lang: Lang, kind_code: &str) -> (&'static str, &'static str) {
    let pt = lang == Lang::PtBr;
    let body = match (kind_code, pt) {
        ("blue_screen", false) => "A blue screen was recorded. Open the app to investigate.",
        ("blue_screen", true) => "Uma tela azul foi registrada. Abra o app para investigar.",
        ("unexpected_shutdown", false) => "An unexpected shutdown was recorded. Open the app to investigate.",
        ("unexpected_shutdown", true) => "Um desligamento inesperado foi registrado. Abra o app para investigar.",
        ("hardware_error", false) => "A hardware error was recorded. Open the app to investigate.",
        ("hardware_error", true) => "Um erro de hardware foi registrado. Abra o app para investigar.",
        ("throttling", false) => "The machine is slowing down from heat. Open the app to see the evidence.",
        ("throttling", true) => "A máquina está reduzindo o desempenho por calor. Abra o app para ver as evidências.",
        ("cpu_sustained", false) => "A program kept the CPU high for a long time. Open the app to investigate.",
        ("cpu_sustained", true) => "Um programa manteve a CPU alta por muito tempo. Abra o app para investigar.",
        ("memory_high", false) => "A program is using a lot of memory. Open the app to investigate.",
        ("memory_high", true) => "Um programa está usando muita memória. Abra o app para investigar.",
        ("unexpected_exit", false) => "A program crashed. Open the app to investigate.",
        ("unexpected_exit", true) => "Um programa falhou. Abra o app para investigar.",
        ("app_hang", false) => "A program stopped responding. Open the app to investigate.",
        ("app_hang", true) => "Um programa parou de responder. Abra o app para investigar.",
        (_, false) => "A new incident was recorded. Open the app to investigate.",
        (_, true) => "Um novo incidente foi registrado. Abra o app para investigar.",
    };
    ("Developer Black Box", body)
}

/// Textos fixos do menu de contexto da bandeja.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuLabels {
    pub open: &'static str,
    pub pause: &'static str,
    pub resume: &'static str,
    pub privacy: &'static str,
    pub quit: &'static str,
    /// Status mostrado antes do primeiro ciclo.
    pub starting_status: &'static str,
    pub starting_tooltip: &'static str,
}

pub fn menu_labels(lang: Lang) -> MenuLabels {
    match lang {
        Lang::En => MenuLabels {
            open: "Open Black Box",
            pause: "Pause recording",
            resume: "Resume recording",
            privacy: "Open privacy settings",
            quit: "Quit",
            starting_status: "Status: starting",
            starting_tooltip: "Developer Black Box: starting",
        },
        Lang::PtBr => MenuLabels {
            open: "Abrir Black Box",
            pause: "Pausar gravação",
            resume: "Retomar gravação",
            privacy: "Abrir configurações de privacidade",
            quit: "Sair",
            starting_status: "Status: iniciando",
            starting_tooltip: "Developer Black Box: iniciando",
        },
    }
}

/// O que o menu de contexto da bandeja deve mostrar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuModel {
    pub status: String,
    pub can_pause: bool,
    pub can_resume: bool,
}

pub fn menu_model(lang: Lang, state: RecorderState, reason: ReasonCode, manually_paused: bool) -> MenuModel {
    let alive = state != RecorderState::ShuttingDown;
    MenuModel {
        status: format!("Status: {}", describe(lang, state, reason)),
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

    const REASONS: [ReasonCode; 9] = [
        ReasonCode::Shutdown,
        ReasonCode::ManualPause,
        ReasonCode::Starting,
        ReasonCode::SensitiveApp,
        ReasonCode::SessionLocked,
        ReasonCode::ContextUnknown,
        ReasonCode::DetectorUnavailable,
        ReasonCode::RecorderFault,
        ReasonCode::RestrictedTest,
    ];

    fn has_portuguese_accent(s: &str) -> bool {
        s.chars().any(|c| "áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ".contains(c))
    }

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

    // ---- idiomas ----

    #[test]
    fn language_codes_round_trip_and_unknown_codes_are_rejected() {
        for l in Lang::ALL {
            assert_eq!(Lang::parse(l.code()), Some(l));
        }
        for bad in ["", "EN", "pt", "pt-br", "fr", "en-US", " en"] {
            assert_eq!(Lang::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_windows_locale_picks_a_language() {
        for pt in ["pt-BR", "pt-PT", "pt", "PT-br", "pt_BR"] {
            assert_eq!(Lang::from_locale(pt), Lang::PtBr, "{pt}");
        }
        for other in ["en-US", "en-GB", "de-DE", "fr-FR", "es-ES", "", "ptolemy", "zh-CN"] {
            assert_eq!(Lang::from_locale(other), Lang::En, "{other}");
        }
    }

    #[test]
    fn english_is_the_default_language() {
        assert_eq!(Lang::default(), Lang::En);
    }

    #[test]
    fn every_state_and_reason_has_text_in_both_languages_and_they_differ() {
        for s in ALL {
            for r in REASONS {
                let en = describe(Lang::En, s, r);
                let pt = describe(Lang::PtBr, s, r);
                assert!(!en.is_empty() && !pt.is_empty(), "{s:?}/{r:?}");
                assert_ne!(en, pt, "{s:?}/{r:?} is not translated");
                assert!(!has_portuguese_accent(&en), "English text with Portuguese accents: {en}");
            }
        }
    }

    #[test]
    fn every_blocked_reason_is_explained_in_both_languages() {
        for r in [
            ReasonCode::SensitiveApp,
            ReasonCode::SessionLocked,
            ReasonCode::ContextUnknown,
            ReasonCode::DetectorUnavailable,
            ReasonCode::RecorderFault,
        ] {
            let en = describe(Lang::En, RecorderState::PrivacyBlocked, r);
            let pt = describe(Lang::PtBr, RecorderState::PrivacyBlocked, r);
            assert!(en.starts_with("Suspended: ") && en.len() > 14, "{en}");
            assert!(pt.starts_with("Suspenso: ") && pt.len() > 12, "{pt}");
        }
        assert_eq!(describe(Lang::En, RecorderState::ManualPause, ReasonCode::ManualPause), "Paused manually");
        assert_eq!(describe(Lang::PtBr, RecorderState::ManualPause, ReasonCode::ManualPause), "Pausado manualmente");
    }

    #[test]
    fn restricted_test_mode_is_green_but_says_so_in_text() {
        assert_eq!(light_for(RecorderState::Recording), Light::Green);
        assert!(describe(Lang::En, RecorderState::Recording, ReasonCode::RestrictedTest).contains("test mode"));
        assert!(describe(Lang::PtBr, RecorderState::Recording, ReasonCode::RestrictedTest).contains("modo de teste"));
        assert_eq!(describe(Lang::En, RecorderState::Recording, ReasonCode::None), "Recording");
        assert_eq!(describe(Lang::PtBr, RecorderState::Recording, ReasonCode::None), "Gravando");
    }

    #[test]
    fn menu_labels_exist_in_both_languages_and_differ() {
        let (en, pt) = (menu_labels(Lang::En), menu_labels(Lang::PtBr));
        for (a, b) in [
            (en.open, pt.open),
            (en.pause, pt.pause),
            (en.resume, pt.resume),
            (en.privacy, pt.privacy),
            (en.quit, pt.quit),
            (en.starting_status, pt.starting_status),
            (en.starting_tooltip, pt.starting_tooltip),
        ] {
            assert!(!a.is_empty() && !b.is_empty());
            assert_ne!(a, b, "{a} is not translated");
            assert!(!has_portuguese_accent(a), "English label with Portuguese accents: {a}");
        }
    }

    #[test]
    fn tooltip_and_status_line_follow_the_language() {
        assert_eq!(tooltip(Lang::En, RecorderState::Recording, ReasonCode::None), "Developer Black Box: Recording");
        assert_eq!(tooltip(Lang::PtBr, RecorderState::Recording, ReasonCode::None), "Developer Black Box: Gravando");
        let en = menu_model(Lang::En, RecorderState::ManualPause, ReasonCode::ManualPause, true);
        let pt = menu_model(Lang::PtBr, RecorderState::ManualPause, ReasonCode::ManualPause, true);
        assert_eq!(en.status, "Status: Paused manually");
        assert_eq!(pt.status, "Status: Pausado manualmente");
    }

    // ---- aviso de incidente ----

    #[test]
    fn every_incident_kind_has_its_own_fixed_notice_in_both_languages() {
        use bb_store::IncidentKind;
        let kinds = [
            IncidentKind::CpuSustained,
            IncidentKind::MemoryHigh,
            IncidentKind::UnexpectedExit,
            IncidentKind::AppHang,
            IncidentKind::UnexpectedShutdown,
            IncidentKind::BlueScreen,
            IncidentKind::HardwareError,
            IncidentKind::Throttling,
        ];
        let generic = |lang| incident_notice(lang, "something-new").1;
        let mut seen = std::collections::HashSet::new();
        for lang in Lang::ALL {
            for k in kinds {
                let (title, body) = incident_notice(lang, k.as_str());
                assert_eq!(title, "Developer Black Box");
                assert_ne!(body, generic(lang), "{k:?} has a specific text in {lang:?}");
                assert!(seen.insert(body), "{k:?} text is not shared with another kind");
            }
        }
        assert_ne!(generic(Lang::En), generic(Lang::PtBr));
    }

    #[test]
    fn a_notice_carries_only_fixed_text_never_a_placeholder_or_a_number() {
        for lang in Lang::ALL {
            for code in [
                "blue_screen",
                "cpu_sustained",
                "memory_high",
                "unexpected_exit",
                "app_hang",
                "throttling",
                "hardware_error",
                "unexpected_shutdown",
                "x",
            ] {
                let (_, body) = incident_notice(lang, code);
                assert!(!body.contains('{') && !body.chars().any(|c| c.is_ascii_digit()), "{code}: {body}");
            }
        }
    }

    // ---- regras do menu (não dependem do idioma) ----

    #[test]
    fn menu_pause_and_resume_availability_is_the_same_in_every_language() {
        for lang in Lang::ALL {
            let rec = menu_model(lang, RecorderState::Recording, ReasonCode::None, false);
            assert!(rec.can_pause && !rec.can_resume);
            let paused = menu_model(lang, RecorderState::ManualPause, ReasonCode::ManualPause, true);
            assert!(!paused.can_pause && paused.can_resume);
            // Retomada pedida mas ainda bloqueada por privacidade: já não está em pausa manual.
            let blocked = menu_model(lang, RecorderState::PrivacyBlocked, ReasonCode::SensitiveApp, false);
            assert!(blocked.can_pause && !blocked.can_resume);
            let down = menu_model(lang, RecorderState::ShuttingDown, ReasonCode::Shutdown, false);
            assert!(!down.can_pause && !down.can_resume);
        }
    }
}
