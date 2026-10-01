//! Privacy Guard: único caminho para criar `ValidatedEvent`.
//!
//! Sem relógio próprio: o chamador informa `now_ms` (monotônico) para tornar o
//! comportamento determinístico e testável. Nenhuma observação é gravada; o
//! Guard mantém em memória apenas o último sinal e o instante em que o
//! contexto passou a ser seguro.

use std::collections::{HashMap, HashSet};

use crate::event::{EventKind, ExeName, ProcessKey, ValidatedEvent};
use crate::exclusion::{ExclusionKind, ExclusionSet};
use crate::state::{
    derive, PrivacyContext, ReasonCode, RecorderInputs, RecorderState, SensitiveReason,
};

/// Navegadores e gerenciadores de senha: protegidos por padrão (política conservadora).
const DEFAULT_PROTECTED: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe",
    "1password.exe", "keepass.exe", "keepassxc.exe", "bitwarden.exe", "lastpass.exe",
    "dashlane.exe",
];

/// Autorização de teste: duração mínima e máxima.
pub const MIN_AUTHORIZATION_MS: u64 = 60_000;
pub const MAX_AUTHORIZATION_MS: u64 = 8 * 3_600_000;

#[derive(Clone, Debug)]
pub struct GuardConfig {
    /// Foreground nestes apps torna o contexto sensível; seus eventos não são gravados
    /// (salvo autorização de teste temporária).
    pub protected_apps: HashSet<ExeName>,
    /// Eventos destes apps nunca são gravados (o contexto não é considerado sensível).
    /// Vence qualquer autorização.
    pub excluded_apps: HashSet<ExeName>,
    /// Exclusões PARCIAIS: de cada programa listado, só os tipos de evento do conjunto são excluídos; o resto
    /// continua sendo gravado. Só estreita o que é gravado de um programa que o usuário escolheu excluir.
    pub partial_exclusions: HashMap<ExeName, ExclusionSet>,
    /// Tempo contínuo de contexto seguro exigido antes de gravar/retomar.
    pub stability_window_ms: u64,
    /// Observação mais velha que isto torna o Guard indisponível (fail-closed).
    pub max_staleness_ms: u64,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            protected_apps: DEFAULT_PROTECTED
                .iter()
                .map(|n| ExeName::new(n).expect("valid default"))
                .collect(),
            excluded_apps: HashSet::new(),
            partial_exclusions: HashMap::new(),
            stability_window_ms: 5_000,
            max_staleness_ms: 10_000,
        }
    }
}

/// Sinal de contexto vindo do collector. Não contém títulos, URLs nem texto.
#[derive(Clone, Debug)]
pub struct Observation {
    pub detector_ok: bool,
    pub session_locked: bool,
    /// `None` = desconhecido, o que conta como não seguro.
    pub foreground: Option<ExeName>,
}

/// Autorização temporária, específica e revogável para coletar dados TÉCNICOS de um app
/// protegido (ex. um navegador em que você testa a sua aplicação web). Fica só em memória:
/// não sobrevive ao fechamento do app. Nunca autoriza conteúdo: o modelo de eventos não tem
/// onde guardá-lo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authorization {
    pub exe: ExeName,
    pub expires_mono_ms: u64,
    /// Início/fim/CPU/memória dos processos do app.
    pub allow_metrics: bool,
    /// Falhas e travamentos do app (Event Log).
    pub allow_crashes: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    /// Só faz sentido autorizar um app que está na lista de protegidos.
    NotProtectedApp,
    /// Uma regra de exclusão vence qualquer autorização.
    ExcludedApp,
    BadDuration,
    NoSource,
}

#[derive(Clone, Copy)]
enum Source {
    Metrics,
    Crashes,
}

pub struct PrivacyGuard {
    config: GuardConfig,
    manual_paused: bool,
    shutting_down: bool,
    last_obs: Option<(u64, Observation)>,
    /// Desde quando o contexto atual (`safe_class`) é contínuo.
    safe_since: Option<u64>,
    safe_class: Option<PrivacyContext>,
    /// Instâncias cujo início foi admitido; métricas/saída de qualquer outra
    /// instância são descartadas (fail-closed). O nome permite reavaliar a permissão a cada
    /// evento (exclusão nova, autorização expirada ou revogada).
    allowed: HashMap<ProcessKey, ExeName>,
    authorizations: Vec<Authorization>,
    next_seq: u64,
}

impl PrivacyGuard {
    pub fn new(config: GuardConfig) -> Self {
        Self {
            config,
            manual_paused: false,
            shutting_down: false,
            last_obs: None,
            safe_since: None,
            safe_class: None,
            allowed: HashMap::new(),
            authorizations: Vec::new(),
            next_seq: 0,
        }
    }

    pub fn config(&self) -> &GuardConfig {
        &self.config
    }

    /// Aplica uma nova configuração na hora. Como a permissão é reavaliada a cada evento,
    /// instâncias de um app agora excluído/protegido deixam de ser gravadas imediatamente.
    /// A janela de estabilidade só vale a partir da próxima observação segura.
    pub fn update_config(&mut self, config: GuardConfig) {
        self.config = config;
        // Uma janela mais longa não pode ser satisfeita retroativamente.
        self.safe_since = None;
        self.safe_class = None;
    }

    // ---- autorizações de teste ----

    /// Autoriza a coleta técnica de um app protegido por um tempo limitado. Reautorizar o
    /// mesmo app substitui a anterior. A janela de estabilidade recomeça.
    pub fn authorize(
        &mut self,
        now_ms: u64,
        exe: ExeName,
        duration_ms: u64,
        allow_metrics: bool,
        allow_crashes: bool,
    ) -> Result<(), AuthError> {
        if self.config.excluded_apps.contains(&exe) {
            return Err(AuthError::ExcludedApp);
        }
        if !self.config.protected_apps.contains(&exe) {
            return Err(AuthError::NotProtectedApp);
        }
        if !(MIN_AUTHORIZATION_MS..=MAX_AUTHORIZATION_MS).contains(&duration_ms) {
            return Err(AuthError::BadDuration);
        }
        if !allow_metrics && !allow_crashes {
            return Err(AuthError::NoSource);
        }
        // Uma exclusão parcial também vence a autorização, fonte por fonte: se tudo o que foi pedido está
        // excluído, não há o que autorizar.
        if (!allow_metrics || self.source_excluded(&exe, Source::Metrics))
            && (!allow_crashes || self.source_excluded(&exe, Source::Crashes))
        {
            return Err(AuthError::ExcludedApp);
        }
        self.authorizations.retain(|a| a.exe != exe);
        self.authorizations.push(Authorization {
            exe,
            expires_mono_ms: now_ms.saturating_add(duration_ms),
            allow_metrics,
            allow_crashes,
        });
        self.safe_since = None;
        self.safe_class = None;
        Ok(())
    }

    /// Revoga na hora. Devolve `true` se havia autorização ativa para o app.
    pub fn revoke_authorization(&mut self, exe: &ExeName) -> bool {
        let before = self.authorizations.len();
        self.authorizations.retain(|a| &a.exe != exe);
        self.authorizations.len() != before
    }

    pub fn revoke_all_authorizations(&mut self) {
        self.authorizations.clear();
    }

    /// Autorizações ainda válidas em `now_ms`.
    pub fn authorizations(&self, now_ms: u64) -> Vec<Authorization> {
        self.authorizations.iter().filter(|a| a.expires_mono_ms > now_ms).cloned().collect()
    }

    /// O programa tem este tipo de evento excluído (exclusão total ou parcial)?
    fn excludes(&self, exe: &ExeName, kind: ExclusionKind) -> bool {
        self.config.excluded_apps.contains(exe) || self.config.partial_exclusions.get(exe).is_some_and(|s| s.contains(kind))
    }

    /// Toda a fonte está excluída? Início/fim e CPU/memória andam juntos (excluir o início exclui as métricas).
    fn source_excluded(&self, exe: &ExeName, source: Source) -> bool {
        match source {
            Source::Metrics => self.excludes(exe, ExclusionKind::Lifecycle),
            Source::Crashes => self.excludes(exe, ExclusionKind::Crashes),
        }
    }

    /// Há autorização ativa que ainda valha para alguma fonte NÃO excluída?
    fn is_authorized(&self, exe: &ExeName, now_ms: u64) -> bool {
        self.authorizations.iter().any(|a| {
            &a.exe == exe
                && a.expires_mono_ms > now_ms
                && ((a.allow_metrics && !self.source_excluded(exe, Source::Metrics))
                    || (a.allow_crashes && !self.source_excluded(exe, Source::Crashes)))
        })
    }

    /// Este programa pode ter eventos deste tipo gravados agora? Excluído (por inteiro ou naquele tipo): nunca.
    /// Protegido: só com autorização ativa que inclua a fonte. Os demais: sim.
    fn permitted(&self, exe: &ExeName, kind: ExclusionKind, now_ms: u64) -> bool {
        if self.excludes(exe, kind) {
            return false;
        }
        if !self.config.protected_apps.contains(exe) {
            return true;
        }
        let source = match kind {
            ExclusionKind::Lifecycle | ExclusionKind::Metrics => Source::Metrics,
            ExclusionKind::Crashes => Source::Crashes,
        };
        self.authorizations.iter().any(|a| {
            &a.exe == exe
                && a.expires_mono_ms > now_ms
                && match source {
                    Source::Metrics => a.allow_metrics,
                    Source::Crashes => a.allow_crashes,
                }
        })
    }

    /// Em modo restrito (primeiro plano = app protegido autorizado), o único app cujos
    /// eventos podem entrar. Só existe enquanto o estado for `Recording`.
    fn restricted_to(&self, now_ms: u64) -> Option<ExeName> {
        let (_, obs) = self.last_obs.as_ref()?;
        match self.classify(obs, now_ms) {
            PrivacyContext::AuthorizedOnly => obs.foreground.clone(),
            _ => None,
        }
    }

    // ---- controle ----

    pub fn is_manually_paused(&self) -> bool {
        self.manual_paused
    }

    pub fn pause_manual(&mut self) {
        self.manual_paused = true;
    }

    /// Libera a pausa manual, mas o estado resultante continua sujeito ao
    /// Guard: pode permanecer bloqueado. Nunca desabilita filtros.
    pub fn resume_manual(&mut self, now_ms: u64) -> (RecorderState, ReasonCode) {
        self.manual_paused = false;
        self.state(now_ms)
    }

    pub fn begin_shutdown(&mut self) {
        self.shutting_down = true;
    }

    /// Roda mesmo com pausa manual ativa: só atualiza estado em memória.
    pub fn observe(&mut self, now_ms: u64, obs: Observation) {
        let previous_stale = match &self.last_obs {
            Some((t, _)) => now_ms.saturating_sub(*t) > self.config.max_staleness_ms,
            None => true,
        };
        if previous_stale {
            self.safe_since = None;
            self.safe_class = None;
        }
        let ctx = self.classify(&obs, now_ms);
        let permitting = matches!(ctx, PrivacyContext::Safe | PrivacyContext::AuthorizedOnly);
        if obs.detector_ok && permitting {
            // Trocar de "seguro" para "só o app autorizado" (ou o contrário) recomeça a janela.
            if self.safe_class != Some(ctx) {
                self.safe_since = Some(now_ms);
                self.safe_class = Some(ctx);
            }
        } else {
            self.safe_since = None;
            self.safe_class = None;
        }
        self.last_obs = Some((now_ms, obs));
    }

    pub fn state(&self, now_ms: u64) -> (RecorderState, ReasonCode) {
        derive(self.inputs(now_ms))
    }

    /// Único ponto de entrada para eventos de atividade. Devolve `None` se o estado não for
    /// `Recording`, se o app estiver excluído/protegido sem autorização, ou (em modo
    /// restrito) se o evento não for do app autorizado.
    pub fn admit(&mut self, now_ms: u64, ts_utc_ms: i64, kind: EventKind) -> Option<ValidatedEvent> {
        if !self.state(now_ms).0.is_recording() {
            return None;
        }
        let restricted = self.restricted_to(now_ms);
        let in_scope = |exe: &ExeName| restricted.as_ref().is_none_or(|r| r == exe);

        match &kind {
            EventKind::ProcessStarted { key, exe_name, .. } => {
                if !self.permitted(exe_name, ExclusionKind::Lifecycle, now_ms) || !in_scope(exe_name) {
                    return None;
                }
                self.allowed.insert(*key, exe_name.clone());
            }
            EventKind::ProcessMetrics { key, .. } => {
                let exe = self.allowed.get(key)?.clone();
                if !self.permitted(&exe, ExclusionKind::Lifecycle, now_ms) {
                    self.allowed.remove(key); // regra nova ou autorização vencida/revogada
                    return None;
                }
                // CPU e memória excluídas (só elas): a instância continua acompanhada para o fim ser gravado.
                if !self.permitted(&exe, ExclusionKind::Metrics, now_ms) || !in_scope(&exe) {
                    return None;
                }
            }
            EventKind::ProcessExited { key, .. } => {
                let exe = self.allowed.get(key)?.clone();
                if !self.permitted(&exe, ExclusionKind::Lifecycle, now_ms) {
                    self.allowed.remove(key);
                    return None;
                }
                if !in_scope(&exe) {
                    return None;
                }
                self.allowed.remove(key);
            }
            // Falhas e travamentos são sobre um aplicativo: mesma regra, fonte própria.
            EventKind::AppCrash { exe_name, .. } | EventKind::AppHang { exe_name } => {
                if !self.permitted(exe_name, ExclusionKind::Crashes, now_ms) || !in_scope(exe_name) {
                    return None;
                }
            }
            // Métricas do sistema não são de um app, mas em modo restrito nada além do
            // app autorizado deve ser coletado.
            EventKind::SystemMetrics { .. } => {
                if restricted.is_some() {
                    return None;
                }
            }
            EventKind::UserMarker { .. } => {}
            // Somente o recorder/Guard produz mudanças de estado.
            EventKind::RecorderStateChanged { .. } => return None,
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        Some(ValidatedEvent::new(seq, ts_utc_ms, kind))
    }

    fn classify(&self, obs: &Observation, now_ms: u64) -> PrivacyContext {
        if obs.session_locked {
            return PrivacyContext::Sensitive(SensitiveReason::SessionLocked);
        }
        match &obs.foreground {
            Some(app) if self.config.protected_apps.contains(app) => {
                if self.is_authorized(app, now_ms) {
                    PrivacyContext::AuthorizedOnly
                } else {
                    PrivacyContext::Sensitive(SensitiveReason::ProtectedApp)
                }
            }
            Some(_) => PrivacyContext::Safe,
            None => PrivacyContext::Unknown,
        }
    }

    fn inputs(&self, now_ms: u64) -> RecorderInputs {
        let (privacy, guard_healthy, starting) = match &self.last_obs {
            None => (PrivacyContext::Unknown, true, true),
            Some((t, obs)) => {
                let fresh = now_ms.saturating_sub(*t) <= self.config.max_staleness_ms;
                let healthy = obs.detector_ok && fresh;
                let privacy = if !healthy {
                    PrivacyContext::Unknown
                } else {
                    match self.classify(obs, now_ms) {
                        c @ (PrivacyContext::Safe | PrivacyContext::AuthorizedOnly) => {
                            let stable = self.safe_class == Some(c)
                                && self.safe_since.is_some_and(|s| {
                                    now_ms.saturating_sub(s) >= self.config.stability_window_ms
                                });
                            if stable { c } else { PrivacyContext::Unknown }
                        }
                        other => other,
                    }
                };
                (privacy, healthy, false)
            }
        };
        RecorderInputs {
            starting,
            shutting_down: self.shutting_down,
            manual_paused: self.manual_paused,
            privacy,
            guard_healthy,
        }
    }
}
