//! Engine: compõe collector → Privacy Guard → recorder.
//!
//! Regras:
//! - O contexto é observado a cada ciclo, mesmo com pausa manual (o Guard segue avaliando).
//! - Fontes de atividade só são consultadas enquanto o estado é `Recording`.
//! - Ao parar de gravar, o estado do `Differ` é esquecido; o que ocorrer no
//!   intervalo nunca é reconstruído ao retomar.

pub mod health;
pub mod inventory;
pub mod incidents;
pub mod settings;
pub mod throttle;

use std::fmt;
use std::time::Duration;

use bb_collector::{
    CollectError, ContextSource, CrashKind, CrashSource, Differ, HealthSource, InventorySource, MetricsConfig, PowerSource, ProcessSource, TelemetrySource,
};
use bb_core::{AuthError, EventKind, ExeName, GuardConfig, PrivacyGuard, ProcessRef, ReasonCode, RecorderState};
use bb_recorder::{Recorder, RecorderError};
use bb_store::{CaptureState, IncidentKind, NewIncident, Severity, Store, StoreError};

pub use incidents::{Detector, Finding, IncidentConfig};
pub use settings::{PartialExclusion, Settings};

#[derive(Debug)]
pub enum EngineError {
    Collect(CollectError),
    Recorder(RecorderError),
    Store(StoreError),
    /// Entrada do usuário rejeitada. Carrega um CÓDIGO (ex. `auth.not_protected`), nunca um texto
    /// de idioma: a interface o traduz.
    Invalid(String),
}

impl EngineError {
    /// Código estável para a interface traduzir. Erros internos não expõem detalhes (podem citar
    /// caminhos); só um código genérico.
    pub fn code(&self) -> String {
        match self {
            EngineError::Invalid(c) => c.clone(),
            EngineError::Store(_) => "store.error".into(),
            EngineError::Recorder(_) => "recorder.error".into(),
            EngineError::Collect(_) => "collect.error".into(),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Collect(e) => write!(f, "{e}"),
            EngineError::Recorder(e) => write!(f, "{e}"),
            EngineError::Store(e) => write!(f, "{e}"),
            EngineError::Invalid(m) => write!(f, "invalid input: {m}"),
        }
    }
}
impl std::error::Error for EngineError {}

impl From<StoreError> for EngineError {
    fn from(e: StoreError) -> Self {
        EngineError::Store(e)
    }
}

/// De onde vêm os dados de saúde da máquina (uma linha da aba "Saúde do sistema").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthSourceId {
    /// Eventos do Event Log (desligamento inesperado, tela azul, hardware, disco, suspensão...).
    EventLog,
    /// BIOS, firmware, Secure Boot, build do Windows e drivers com problema.
    Inventory,
    /// Tomada e bateria.
    Power,
    /// Contadores de desempenho (temperatura, CPU, memória, disco, rede, GPU).
    Telemetry,
}

/// Estado de uma fonte. `Unavailable` (não existe nesta máquina ou não pôde ser lida) NUNCA é `Attention`: indisponível
/// não é alarme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceState {
    /// Lendo e sem nada a destacar.
    Ok,
    /// Lendo, e há algo que merece olhar (dispositivo com problema, bateria baixa, throttling prolongado).
    Attention,
    /// Não existe nesta máquina ou a última leitura falhou.
    Unavailable,
    /// Ainda não houve uma leitura com sucesso (por exemplo, o app acabou de abrir ou está em modo de teste restrito).
    Waiting,
    /// A pausa manual está impedindo a leitura agora.
    Paused,
    /// Desligada pela pessoa (só a telemetria tem chave).
    Off,
}

/// Resultado de uma exportação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportResult {
    pub path: std::path::PathBuf,
    pub events: usize,
    /// Eventos removidos pela nova filtragem de privacidade.
    pub dropped: usize,
}

/// Autorização de teste ainda válida, para exibição.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationInfo {
    pub exe: String,
    pub remaining_ms: u64,
    pub allow_metrics: bool,
    pub allow_crashes: bool,
}

struct IncidentState {
    store: Store,
    detector: Detector,
}

impl From<RecorderError> for EngineError {
    fn from(e: RecorderError) -> Self {
        EngineError::Recorder(e)
    }
}
impl From<CollectError> for EngineError {
    fn from(e: CollectError) -> Self {
        EngineError::Collect(e)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct TickReport {
    pub state: RecorderState,
    pub reason: ReasonCode,
    pub persisted: usize,
    pub dropped_by_guard: usize,
    pub incidents_opened: usize,
}

pub struct Engine<P: ProcessSource, C: ContextSource> {
    guard: PrivacyGuard,
    recorder: Recorder,
    procs: P,
    ctx: C,
    differ: Differ,
    was_recording: bool,
    /// Último instante (UTC ms) em que um ciclo gravou com o estado `Recording`.
    last_recording_utc: Option<i64>,
    /// Primeiro instante em que o engine observou o sistema (UTC ms).
    first_seen_utc: Option<i64>,
    /// Início do intervalo sem gravação: processos nascidos depois disso são adotados em silêncio.
    gap_start_utc: Option<i64>,
    incidents: Option<IncidentState>,
    /// Última tentativa de coletar/gravar falhou; volta a `false` no primeiro ciclo bem-sucedido.
    fault: bool,
    settings: Settings,
    crashes: Option<Box<dyn CrashSource + Send>>,
    crash_state: CrashState,
    health: Option<Box<dyn HealthSource + Send>>,
    health_win: health::HealthWindow,
    /// Última consulta de saúde (relógio monotônico). `None` = consultar assim que puder.
    health_last_poll_mono: Option<u64>,
    /// A última consulta falhou: a fonte aparece como indisponível, sem erro nem alarme.
    health_unavailable: bool,
    shutting_down: bool,
    inventory: Option<Box<dyn InventorySource + Send>>,
    inventory_base: inventory::Baseline,
    /// Tipos de incidente automático à espera de aviso ao usuário (hora UTC de abertura). Só o tipo, nada mais.
    notices: Vec<(IncidentKind, i64)>,
    /// Próxima leitura do inventário (relógio monotônico). `None` = ler assim que a saúde puder ser lida.
    inventory_next_mono: Option<u64>,
    inventory_unavailable: bool,
    power: Option<Box<dyn PowerSource + Send>>,
    power_tracker: bb_collector::power::PowerTracker,
    /// Próxima leitura de energia (relógio monotônico). `None` = ler assim que a saúde puder ser lida.
    power_next_mono: Option<u64>,
    /// Sem bateria ou leitura falha: a fonte aparece como indisponível, sem erro nem alarme.
    power_unavailable: bool,
    telemetry: Option<Box<dyn TelemetrySource + Send>>,
    throttle: throttle::ThrottleDetector,
    /// Próxima amostra de telemetria (relógio monotônico). `None` = amostrar assim que puder.
    telemetry_next_mono: Option<u64>,
    /// A fonte de contadores falhou ou não leu nada: aparece como indisponível, sem erro nem alarme.
    telemetry_unavailable: bool,
    /// Cada fonte já teve ao menos uma leitura com sucesso? (antes disso o estado é "aguardando", não "ok")
    health_read_ok: bool,
    inventory_read_ok: bool,
    power_read_ok: bool,
    telemetry_read_ok: bool,
    /// Última leitura de energia (mesmo as não gravadas), para destacar bateria baixa.
    power_last: Option<bb_collector::PowerReading>,
}

/// Chaves (cifradas) da marca d'água de saúde: até onde a última execução leu, e se terminou lendo.
const HEALTH_CURSOR_KEY: &str = "health.cursor";
const HEALTH_ACTIVE_KEY: &str = "health.active";
/// Referência do inventário (números por item, cifrada no armazenamento).
/// Um aviso que esperou mais que isto (por exemplo, durante um bloqueio de privacidade) já não serve e é descartado.
const NOTICE_MAX_AGE_MS: i64 = 30 * 60_000;
const INVENTORY_KEY: &str = "health.inventory";
/// Intervalo entre leituras do inventário (relógio monotônico): detecta, por exemplo, um dispositivo que passou a falhar.
const INVENTORY_INTERVAL_MS: u64 = 10 * 60_000;
/// Intervalo entre leituras de energia (relógio monotônico). Só variações relevantes viram evento.
const POWER_INTERVAL_MS: u64 = 60_000;
/// Intervalo entre amostras de telemetria (relógio monotônico): ~1 amostra a cada 30 s.
const TELEMETRY_INTERVAL_MS: u64 = 30_000;
/// Bateria na bateria (fora da tomada) até esta carga é destacada como "atenção".
const LOW_BATTERY_PCT: u8 = 10;

/// Uma falha registrada pelo Windows só interessa se foi registrada DURANTE uma gravação.
/// O que ocorreu numa pausa ou bloqueio nunca é reconstruído depois.
#[derive(Default)]
struct CrashState {
    /// Início da sequência de gravação atual (UTC ms). `None` = não está gravando.
    stretch_start: Option<i64>,
    /// Próxima consulta parte daqui (com sobreposição, para não perder registros tardios).
    cursor: i64,
    /// Registros já processados dentro da janela de sobreposição (evita duplicar).
    seen: Vec<(i64, CrashKind, String)>,
}

/// Sobreposição entre consultas ao Event Log: o Windows grava o evento alguns instantes depois.
const CRASH_OVERLAP_MS: i64 = 3_000;

impl<P: ProcessSource, C: ContextSource> Engine<P, C> {
    pub fn new(
        guard_cfg: GuardConfig,
        metrics: MetricsConfig,
        recorder: Recorder,
        procs: P,
        ctx: C,
        ncpu: usize,
    ) -> Self {
        Self {
            guard: PrivacyGuard::new(guard_cfg),
            recorder,
            procs,
            ctx,
            differ: Differ::new(metrics, ncpu),
            was_recording: false,
            last_recording_utc: None,
            first_seen_utc: None,
            gap_start_utc: None,
            incidents: None,
            fault: false,
            settings: Settings::default(),
            crashes: None,
            crash_state: CrashState::default(),
            health: None,
            health_win: health::HealthWindow::new(),
            health_last_poll_mono: None,
            health_unavailable: false,
            shutting_down: false,
            inventory: None,
            inventory_base: inventory::Baseline::default(),
            notices: Vec::new(),
            inventory_next_mono: None,
            inventory_unavailable: false,
            power: None,
            power_tracker: bb_collector::power::PowerTracker::new(),
            power_next_mono: None,
            power_unavailable: false,
            telemetry: None,
            throttle: throttle::ThrottleDetector::new(),
            telemetry_next_mono: None,
            telemetry_unavailable: false,
            health_read_ok: false,
            inventory_read_ok: false,
            power_read_ok: false,
            telemetry_read_ok: false,
            power_last: None,
        }
    }

    /// Liga a leitura de falhas e travamentos (Windows Event Log).
    pub fn set_crash_source(&mut self, src: Box<dyn CrashSource + Send>) {
        self.crashes = Some(src);
    }

    /// Liga a leitura de eventos de saúde da máquina (Windows Event Log `System`). Chame DEPOIS de `enable_incidents`: a
    /// marca d'água da execução anterior vem do armazenamento cifrado. Se a execução anterior terminou lendo, o intervalo
    /// em que o app esteve fechado (até 7 dias) entra na primeira leitura; senão (pausa manual, primeira vez) começa de agora.
    pub fn set_health_source(&mut self, src: Box<dyn HealthSource + Send>, utc_ms: i64) {
        self.health_win = match self.incidents.as_ref().map(|i| &i.store) {
            Some(store) => {
                let active = store.get_setting(HEALTH_ACTIVE_KEY).ok().flatten().as_deref() == Some("1");
                let cursor = store.get_setting(HEALTH_CURSOR_KEY).ok().flatten().and_then(|v| v.parse::<i64>().ok());
                match (active, cursor) {
                    (true, Some(c)) => health::HealthWindow::with_backlog(c, utc_ms),
                    _ => health::HealthWindow::new(),
                }
            }
            None => health::HealthWindow::new(),
        };
        self.health = Some(src);
    }

    /// Liga a leitura do inventário da máquina (BIOS, firmware, Secure Boot, build, drivers com problema). Chame DEPOIS de
    /// `enable_incidents`: a referência da execução anterior vem do armazenamento cifrado. Só MUDANÇAS viram evento.
    pub fn set_inventory_source(&mut self, src: Box<dyn InventorySource + Send>) {
        self.inventory_base = self
            .incidents
            .as_ref()
            .and_then(|i| i.store.get_setting(INVENTORY_KEY).ok().flatten())
            .map(|s| inventory::Baseline::parse(&s))
            .unwrap_or_default();
        self.inventory = Some(src);
    }

    /// Liga a leitura de energia e bateria (tomada e porcentagem de carga).
    pub fn set_power_source(&mut self, src: Box<dyn PowerSource + Send>) {
        self.power = Some(src);
    }

    /// Liga a telemetria contínua de desempenho do sistema (contadores PDH). Só amostra enquanto a configuração
    /// `telemetry_enabled` estiver ligada (padrão) e a saúde puder ser lida.
    pub fn set_telemetry_source(&mut self, src: Box<dyn TelemetrySource + Send>) {
        self.telemetry = Some(src);
    }

    /// A fonte de contadores falhou ou não leu nenhum contador: mostrada como indisponível.
    pub fn telemetry_unavailable(&self) -> bool {
        self.telemetry_unavailable
    }

    /// Estado de cada fonte de saúde agora. Só mostra o que já foi lido; nada aqui lê nada novo.
    pub fn health_sources(&self, mono_ms: u64) -> Vec<(HealthSourceId, SourceState)> {
        let allowed = self.guard.health_allowed(mono_ms);
        let state = |present: bool, enabled: bool, unavailable: bool, read_ok: bool, attention: bool| -> SourceState {
            if !present {
                SourceState::Unavailable
            } else if !enabled {
                SourceState::Off
            } else if !allowed {
                if self.guard.is_manually_paused() { SourceState::Paused } else { SourceState::Waiting }
            } else if unavailable {
                SourceState::Unavailable
            } else if !read_ok {
                SourceState::Waiting
            } else if attention {
                SourceState::Attention
            } else {
                SourceState::Ok
            }
        };
        let devices_with_problem = self.inventory_base.get(bb_core::InventoryItem::DeviceProblemCount).is_some_and(|n| n > 0);
        let battery_low = self
            .power_last
            .is_some_and(|p| p.ac == Some(bb_core::AcLine::Offline) && p.charge_percent.is_some_and(|c| c <= LOW_BATTERY_PCT));
        vec![
            (HealthSourceId::EventLog, state(self.health.is_some(), self.settings.health_log_enabled, self.health_unavailable, self.health_read_ok, false)),
            (HealthSourceId::Inventory, state(self.inventory.is_some(), self.settings.inventory_enabled, self.inventory_unavailable, self.inventory_read_ok, devices_with_problem)),
            (HealthSourceId::Power, state(self.power.is_some(), self.settings.power_enabled, self.power_unavailable, self.power_read_ok, battery_low)),
            (
                HealthSourceId::Telemetry,
                state(self.telemetry.is_some(), self.settings.telemetry_enabled, self.telemetry_unavailable, self.telemetry_read_ok, self.throttle.sustained()),
            ),
        ]
    }

    /// Há throttling térmico prolongado (limite passivo abaixo de 100 % com carga alta por ~5 min)? Sinal para a #73.
    pub fn throttling_sustained(&self) -> bool {
        self.throttle.sustained()
    }

    /// Sem bateria ou leitura falha: a fonte de energia é mostrada como indisponível.
    pub fn power_unavailable(&self) -> bool {
        self.power_unavailable
    }

    /// A última leitura do inventário falhou: a fonte aparece como indisponível, sem erro.
    pub fn inventory_unavailable(&self) -> bool {
        self.inventory_unavailable
    }

    /// A leitura de saúde falhou na última tentativa (canal indisponível): a fonte é mostrada como indisponível.
    pub fn health_unavailable(&self) -> bool {
        self.health_unavailable
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Autoriza, por tempo limitado, a coleta TÉCNICA de um app protegido (ex. um navegador em
    /// que você testa a sua aplicação web). Fica só em memória e nunca autoriza conteúdo.
    pub fn authorize_app(
        &mut self,
        exe: &str,
        minutes: u64,
        allow_metrics: bool,
        allow_crashes: bool,
        mono_ms: u64,
        utc_ms: i64,
    ) -> Result<(), EngineError> {
        let name = ExeName::new(exe.trim().to_lowercase().as_str())
            .map_err(|_| EngineError::Invalid("auth.bad_name".into()))?;
        self.guard
            .authorize(mono_ms, name, minutes.saturating_mul(60_000), allow_metrics, allow_crashes)
            .map_err(|e| {
                EngineError::Invalid(
                    match e {
                        AuthError::NotProtectedApp => "auth.not_protected",
                        AuthError::ExcludedApp => "auth.excluded",
                        AuthError::BadDuration => "auth.bad_duration",
                        AuthError::NoSource => "auth.no_source",
                    }
                    .into(),
                )
            })?;
        self.log_authorization_change(utc_ms, "added")
    }

    /// Revoga na hora. A mudança é registrada só como "removed", sem o nome do app.
    pub fn revoke_authorization(&mut self, exe: &str, utc_ms: i64) -> Result<bool, EngineError> {
        let Ok(name) = ExeName::new(exe.trim().to_lowercase().as_str()) else { return Ok(false) };
        let existed = self.guard.revoke_authorization(&name);
        if existed {
            self.log_authorization_change(utc_ms, "removed")?;
        }
        Ok(existed)
    }

    pub fn authorizations(&self, mono_ms: u64) -> Vec<AuthorizationInfo> {
        self.guard
            .authorizations(mono_ms)
            .into_iter()
            .map(|a| AuthorizationInfo {
                exe: a.exe.as_str().to_owned(),
                remaining_ms: a.expires_mono_ms.saturating_sub(mono_ms),
                allow_metrics: a.allow_metrics,
                allow_crashes: a.allow_crashes,
            })
            .collect()
    }

    fn log_authorization_change(&self, utc_ms: i64, change: &str) -> Result<(), EngineError> {
        if let Some(i) = self.incidents.as_ref() {
            i.store.log_config_change(utc_ms, "authorizations", change)?;
        }
        Ok(())
    }

    /// Carrega as configurações salvas e as aplica (sem registrar mudança no histórico).
    /// Sem armazenamento ligado, ou com dados inválidos, ficam os padrões conservadores.
    pub fn load_settings(&mut self) -> Result<(), EngineError> {
        let loaded = match self.incidents.as_ref() {
            Some(i) => Settings::load(&i.store),
            None => Settings::default(),
        };
        self.install_settings(loaded)
    }

    /// Valida, aplica na hora (o Guard e a retenção mudam imediatamente) e persiste.
    /// O histórico registra só a chave e o tipo da mudança, nunca o valor.
    pub fn apply_settings(&mut self, new: Settings, utc_ms: i64) -> Result<(), EngineError> {
        let new = new.normalized();
        new.validate().map_err(EngineError::Invalid)?;
        let changes = self.settings.diff(&new);
        self.install_settings(new.clone())?;
        if let Some(i) = self.incidents.as_ref() {
            new.save(&i.store)?;
            for (key, change) in changes {
                i.store.log_config_change(utc_ms, key, change)?;
            }
        }
        Ok(())
    }

    fn install_settings(&mut self, s: Settings) -> Result<(), EngineError> {
        self.guard.update_config(s.guard_config());
        let mut rc = self.recorder.config().clone();
        rc.max_total_bytes = s.retention_max_mb << 20;
        rc.max_age = Some(Duration::from_secs(s.retention_max_hours * 3600));
        self.recorder.set_config(rc)?;
        self.settings = s;
        Ok(())
    }

    /// A exportação de um incidente já refiltrada pelas regras de privacidade de AGORA (em memória; nada é escrito).
    fn export_doc(&self, id: i64, mono_ms: u64, utc_ms: i64) -> Result<bb_query::ExportDoc, EngineError> {
        let Some(inc) = self.incidents.as_ref() else {
            return Err(EngineError::Invalid("store.unavailable".into()));
        };
        let cfg = self.guard.config();
        let rules = bb_query::ExportRules {
            excluded: cfg.excluded_apps.iter().map(|n| n.as_str().to_owned()).collect(),
            protected: cfg.protected_apps.iter().map(|n| n.as_str().to_owned()).collect(),
            authorized_now: self.guard.authorizations(mono_ms).into_iter().map(|a| a.exe.as_str().to_owned()).collect(),
            partial: cfg.partial_exclusions.iter().map(|(n, set)| (n.as_str().to_owned(), *set)).collect(),
            trees: cfg.excluded_trees.iter().filter(|n| cfg.excluded_apps.contains(*n)).map(|n| n.as_str().to_owned()).collect(),
            pre_window_ms: inc.detector.config().pre_window_ms,
            // As amostras de contadores só saem se a telemetria está LIGADA agora: desligou depois de gravar, não saem.
            include_samples: self.settings.telemetry_enabled,
        };
        bb_query::export_incident(&self.recorder, &inc.store, id, &rules, utc_ms)
            .ok_or_else(|| EngineError::Invalid("export.not_found".into()))
    }

    /// Exporta um incidente para `out_dir` (criada se preciso), refiltrando pelas regras de
    /// privacidade de AGORA. O arquivo sai da cifra; o nome é gerado (id e horário), nunca
    /// derivado de dados do incidente. Devolve o caminho e quantos eventos a filtragem removeu.
    pub fn export_incident(
        &mut self,
        id: i64,
        out_dir: &std::path::Path,
        mono_ms: u64,
        utc_ms: i64,
    ) -> Result<ExportResult, EngineError> {
        let doc = self.export_doc(id, mono_ms, utc_ms)?;
        std::fs::create_dir_all(out_dir).map_err(|_| EngineError::Invalid("export.folder".into()))?;
        let path = out_dir.join(format!("incident-{id}-{utc_ms}.json"));
        let json = serde_json::to_vec_pretty(&doc).map_err(|_| EngineError::Invalid("export.write".into()))?;
        std::fs::write(&path, json).map_err(|_| EngineError::Invalid("export.write".into()))?;
        Ok(ExportResult { path, events: doc.events.len(), dropped: doc.dropped_events })
    }

    /// Resumo em texto para relatar um bug, montado a partir da MESMA exportação refiltrada (nunca direto dos dados
    /// gravados). O build do Windows só entra se o inventário está ligado e já o leu.
    pub fn bug_report_summary(
        &self,
        id: i64,
        app_version: &str,
        lang: bb_query::SummaryLang,
        mono_ms: u64,
        utc_ms: i64,
    ) -> Result<String, EngineError> {
        let doc = self.export_doc(id, mono_ms, utc_ms)?;
        let os_build = if self.settings.inventory_enabled { self.inventory_base.get(bb_core::InventoryItem::OsBuild) } else { None };
        Ok(bb_query::bug_report_summary(&doc, app_version, os_build, lang))
    }

    /// Exclui atividade gravada. As evidências de incidentes só saem com `include_preserved`,
    /// e nesse caso os incidentes (que ficariam sem evidência) também são removidos.
    pub fn delete_activity(&mut self, include_preserved: bool) -> Result<usize, EngineError> {
        let removed = self.recorder.delete_segments(include_preserved)?;
        if include_preserved {
            if let Some(i) = self.incidents.as_ref() {
                for inc in i.store.list_incidents()? {
                    i.store.delete_incident(inc.id)?;
                }
            }
        }
        Ok(removed)
    }

    /// Liga o Incident Engine: detecção de anomalias e captura de incidentes.
    pub fn enable_incidents(&mut self, store: Store, cfg: IncidentConfig) {
        self.incidents = Some(IncidentState { store, detector: Detector::new(cfg) });
    }

    pub fn store(&self) -> Option<&Store> {
        self.incidents.as_ref().map(|i| &i.store)
    }

    /// Captura manual: preserva a janela anterior agora e a posterior quando vencer.
    /// Não coleta nada novo: com a gravação pausada só preserva o que já existia.
    pub fn capture_manual(&mut self, utc_ms: i64) -> Result<i64, EngineError> {
        self.open_incident(IncidentKind::Manual, Severity::Info, None, "manual".into(), utc_ms)
    }

    /// Remove o incidente e libera a marca de preservação dos segmentos que
    /// nenhum outro incidente referencia (a retenção volta a poder removê-los).
    pub fn delete_incident(&mut self, id: i64) -> Result<(), EngineError> {
        let Some(inc) = self.incidents.as_ref() else { return Ok(()) };
        let all = inc.store.list_incidents()?;
        let Some(target) = all.iter().find(|i| i.id == id) else { return Ok(()) };
        for seg in &target.segments {
            let shared = all.iter().any(|o| o.id != id && o.segments.contains(seg));
            if !shared {
                self.recorder.unpreserve(*seg)?;
            }
        }
        inc.store.delete_incident(id)?;
        Ok(())
    }

    fn open_incident(
        &mut self,
        kind: IncidentKind,
        severity: Severity,
        exe_name: Option<String>,
        summary: String,
        utc_ms: i64,
    ) -> Result<i64, EngineError> {
        let Some(inc) = self.incidents.as_ref() else {
            return Err(EngineError::Invalid("store.unavailable".into()));
        };
        let cfg = inc.detector.config().clone();
        let id = inc.store.create_incident(&NewIncident {
            kind,
            severity,
            created_utc_ms: utc_ms,
            exe_name,
            summary,
            post_until_utc_ms: utc_ms + cfg.post_window_ms,
        })?;
        self.queue_notice(kind, utc_ms);
        // Preserva já a janela anterior, para a retenção não removê-la antes do fim da captura.
        self.recorder.seal()?;
        let segs = self.recorder.segments_overlapping(utc_ms - cfg.pre_window_ms, utc_ms)?;
        for s in &segs {
            self.recorder.preserve(*s)?;
        }
        if let Some(inc) = self.incidents.as_ref() {
            inc.store.set_segments(id, &segs)?;
        }
        Ok(id)
    }

    /// Guarda o tipo de um incidente automático para o aviso ao usuário, se ele ligou o aviso. A captura manual não avisa
    /// (foi a própria pessoa que pediu) e um tipo já à espera não se repete.
    fn queue_notice(&mut self, kind: IncidentKind, utc_ms: i64) {
        if !self.settings.notify_incidents || kind == IncidentKind::Manual || self.notices.iter().any(|(k, _)| *k == kind) {
            return;
        }
        self.notices.push((kind, utc_ms));
    }

    /// Tipos de incidente a avisar AGORA, uma vez cada. Só avisa quando a gravação está de fato ativa: durante um bloqueio
    /// de privacidade (um navegador ou gerenciador de senhas na frente) o aviso espera, para não aparecer por cima dele;
    /// pausa manual ou aviso desligado descartam o que estava na fila; o que esperou mais de `NOTICE_MAX_AGE_MS` também.
    pub fn take_incident_notices(&mut self, mono_ms: u64, utc_ms: i64) -> Vec<IncidentKind> {
        if !self.settings.notify_incidents || self.guard.is_manually_paused() {
            self.notices.clear();
            return Vec::new();
        }
        self.notices.retain(|(_, at)| utc_ms.saturating_sub(*at) <= NOTICE_MAX_AGE_MS);
        if self.state(mono_ms).0 != RecorderState::Recording {
            return Vec::new();
        }
        std::mem::take(&mut self.notices).into_iter().map(|(k, _)| k).collect()
    }

    /// Conclui capturas cuja janela posterior venceu (ou todas, se `force`).
    fn finalize_captures(&mut self, utc_ms: i64, force: bool) -> Result<(), EngineError> {
        let Some(inc) = self.incidents.as_ref() else { return Ok(()) };
        let pre = inc.detector.config().pre_window_ms;
        for pending in inc.store.pending_captures()? {
            if !force && utc_ms < pending.post_until_utc_ms {
                continue;
            }
            self.recorder.seal()?;
            let to = pending.post_until_utc_ms.min(utc_ms);
            let segs = self.recorder.segments_overlapping(pending.created_utc_ms - pre, to)?;
            for s in &segs {
                self.recorder.preserve(*s)?;
            }
            if let Some(inc) = self.incidents.as_ref() {
                inc.store.set_segments(pending.id, &segs)?;
                inc.store.set_capture_state(pending.id, CaptureState::Preserved)?;
            }
        }
        Ok(())
    }

    /// Estado real. Uma falha de coleta/gravação sobrepõe `Recording` (o ícone não fica
    /// verde), mas nunca mascara pausa manual, bloqueio de privacidade ou encerramento.
    pub fn state(&self, mono_ms: u64) -> (RecorderState, ReasonCode) {
        let (state, reason) = self.guard.state(mono_ms);
        if self.fault && state.is_recording() {
            return (RecorderState::SafetyFault, ReasonCode::RecorderFault);
        }
        (state, reason)
    }

    /// Quantas vezes cada regra de exclusão ativa deixou algo de fora desde que o app abriu: só o nome da regra e um número.
    pub fn omitted_counts(&self) -> Vec<(String, u64)> {
        self.guard.omitted_counts().into_iter().map(|(e, n)| (e.as_str().to_owned(), n)).collect()
    }

    pub fn is_manually_paused(&self) -> bool {
        self.guard.is_manually_paused()
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    pub fn recorder_mut(&mut self) -> &mut Recorder {
        &mut self.recorder
    }

    /// A pausa tem efeito imediato: o próximo `tick` já não consulta nem grava atividade.
    pub fn pause(&mut self) {
        self.guard.pause_manual();
        self.stop_recording();
    }

    /// Libera a pausa manual; o resultado ainda depende do Guard e pode continuar bloqueado.
    pub fn resume(&mut self, mono_ms: u64) -> (RecorderState, ReasonCode) {
        self.guard.resume_manual(mono_ms)
    }

    /// Sela o journal e encerra. Depois disto nada mais é gravado.
    pub fn shutdown(&mut self) -> Result<(), EngineError> {
        // Encerrar normalmente NÃO conta como pausa: a marca d'água segue "lendo", e o próximo início cobre o intervalo fechado.
        self.shutting_down = true;
        self.guard.begin_shutdown();
        self.stop_recording();
        self.recorder.seal()?;
        // Encerrar não deixa capturas pela metade: a janela posterior é truncada.
        self.finalize_captures(i64::MAX, true)?;
        Ok(())
    }

    fn stop_recording(&mut self) {
        if self.was_recording {
            self.gap_start_utc = self.last_recording_utc;
            self.was_recording = false;
        } else if self.gap_start_utc.is_none() && self.last_recording_utc.is_none() {
            // Nunca gravou ainda: tudo que nasce depois de começarmos a observar,
            // e antes da primeira gravação, é tratado como intervalo sem gravação.
            self.gap_start_utc = self.first_seen_utc;
        }
        self.differ.reset();
        self.crash_state = CrashState::default();
    }

    /// Um ciclo. `mono_ms`: relógio monotônico; `utc_ms`: relógio de parede.
    ///
    /// Se coletar ou gravar falhar, o estado passa a `SafetyFault` (ícone vermelho) e a
    /// lacuna é tratada como intervalo sem gravação; o próximo ciclo bem-sucedido recupera.
    pub fn tick(&mut self, mono_ms: u64, utc_ms: i64) -> Result<TickReport, EngineError> {
        match self.tick_inner(mono_ms, utc_ms) {
            Ok(mut report) => {
                if report.state.is_recording() {
                    self.fault = false;
                }
                let (state, reason) = self.state(mono_ms);
                report.state = state;
                report.reason = reason;
                Ok(report)
            }
            Err(e) => {
                self.fault = true;
                self.stop_recording();
                Err(e)
            }
        }
    }

    /// Consulta o Event Log e devolve só o que foi registrado durante esta sequência de gravação.
    /// Não confia na fonte: o filtro por horário e a deduplicação são feitos aqui.
    fn poll_crashes(&mut self, utc_ms: i64) -> Result<Vec<(i64, EventKind)>, EngineError> {
        let (Some(src), Some(start)) = (self.crashes.as_mut(), self.crash_state.stretch_start) else {
            return Ok(Vec::new());
        };
        let records = src.poll(self.crash_state.cursor)?;
        let mut out = Vec::new();
        for r in records {
            if r.ts_utc_ms < start || r.ts_utc_ms < self.crash_state.cursor {
                continue; // anterior a esta gravação (ex. durante uma pausa): nunca reconstruído
            }
            let key = (r.ts_utc_ms, r.kind, r.exe_name.as_str().to_owned());
            if self.crash_state.seen.contains(&key) {
                continue;
            }
            self.crash_state.seen.push(key);
            let kind = match r.kind {
                CrashKind::Crash => EventKind::AppCrash { exe_name: r.exe_name, exception_code: r.exception_code.unwrap_or(0) },
                CrashKind::Hang => EventKind::AppHang { exe_name: r.exe_name },
            };
            out.push((r.ts_utc_ms, kind));
        }
        self.crash_state.cursor = (utc_ms - CRASH_OVERLAP_MS).max(start);
        let cursor = self.crash_state.cursor;
        self.crash_state.seen.retain(|(ts, _, _)| *ts >= cursor);
        Ok(out)
    }

    fn save_health_marks(&self, active: bool, cursor: Option<i64>) {
        // Melhor esforço: falhar em salvar a marca d'água nunca derruba o ciclo.
        if let Some(i) = self.incidents.as_ref() {
            let _ = i.store.set_setting(HEALTH_ACTIVE_KEY, if active { "1" } else { "0" });
            if let Some(c) = cursor {
                let _ = i.store.set_setting(HEALTH_CURSOR_KEY, &c.to_string());
            }
        }
    }

    /// Grava um evento de saúde já admitido pelo Guard e o entrega ao detector (que só enxerga o que foi admitido): tela azul,
    /// desligamento inesperado e erro de hardware abrem um incidente, com a janela de evidências antes e depois como os demais.
    fn commit_health(&mut self, ev: bb_core::ValidatedEvent, utc_ms: i64) -> Result<(), EngineError> {
        self.recorder.append(&ev)?;
        let findings = match self.incidents.as_mut() {
            Some(inc) => inc.detector.observe(ev.kind(), ev.ts_utc_ms()).into_iter().collect::<Vec<_>>(),
            None => Vec::new(),
        };
        for f in findings {
            self.open_incident(f.kind, f.severity, f.exe_name.map(|e| e.as_str().to_owned()), f.summary, utc_ms)?;
        }
        Ok(())
    }

    /// Amostra os contadores de desempenho (~30 s) e grava uma amostra de números agregados. Mesma porta e mesmas travas
    /// da saúde, mais a chave `telemetry_enabled`: desligada, nada é lido nem gravado. Amostra sem nenhum número =
    /// fonte indisponível, nada gravado.
    fn telemetry_tick(&mut self, mono_ms: u64, utc_ms: i64) -> Result<usize, EngineError> {
        if self.telemetry.is_none() {
            return Ok(0);
        }
        if !self.settings.telemetry_enabled || !self.guard.health_allowed(mono_ms) {
            self.telemetry_next_mono = None; // ao voltar, amostra de novo na hora (referências de taxa recomeçam)
            self.throttle.reset();
            return Ok(0);
        }
        if self.telemetry_next_mono.is_some_and(|t| mono_ms < t) {
            return Ok(0);
        }
        self.telemetry_next_mono = Some(mono_ms.saturating_add(TELEMETRY_INTERVAL_MS));
        let Some(src) = self.telemetry.as_mut() else { return Ok(0) };
        let sample = match src.sample() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) | Err(_) => {
                self.telemetry_unavailable = true;
                self.throttle.reset();
                return Ok(0);
            }
        };
        self.telemetry_unavailable = false;
        self.telemetry_read_ok = true;
        let was_sustained = self.throttle.sustained();
        let now_sustained = self.throttle.observe(&sample);
        let admitted = self.guard.admit_health(mono_ms, utc_ms, EventKind::HealthSample(sample));
        let persisted = match admitted {
            Some(ev) => {
                self.commit_health(ev, utc_ms)?;
                1
            }
            None => 0,
        };
        // Throttling que passa a ser prolongado: um incidente de aviso (a borda, não cada amostra).
        if now_sustained && !was_sustained {
            let finding = match self.incidents.as_mut() {
                Some(inc) => inc.detector.throttling_finding(sample.passive_limit_pct, sample.cpu_load_pct, utc_ms),
                None => None,
            };
            if let Some(f) = finding {
                self.open_incident(f.kind, f.severity, None, f.summary, utc_ms)?;
            }
        }
        Ok(persisted)
    }

    /// Lê a energia (a cada minuto) e grava a primeira leitura, mudanças de tomada e variações de carga. Mesma porta e
    /// mesmas travas da saúde. Sem bateria, a fonte fica indisponível e nada é gravado.
    fn power_tick(&mut self, mono_ms: u64, utc_ms: i64) -> Result<usize, EngineError> {
        if self.power.is_none() {
            return Ok(0);
        }
        if !self.settings.power_enabled {
            // Desligada pela pessoa: nada é lido, e ao ligar de novo a primeira leitura é a de agora.
            self.power_next_mono = None;
            self.power_tracker.reset();
            self.power_last = None;
            self.power_read_ok = false;
            return Ok(0);
        }
        if !self.guard.health_allowed(mono_ms) {
            self.power_next_mono = None; // depois de uma pausa, a primeira leitura volta a ser gravada
            self.power_tracker.reset();
            return Ok(0);
        }
        if self.power_next_mono.is_some_and(|t| mono_ms < t) {
            return Ok(0);
        }
        self.power_next_mono = Some(mono_ms.saturating_add(POWER_INTERVAL_MS));
        let Some(src) = self.power.as_mut() else { return Ok(0) };
        let reading = match src.read() {
            Ok(Some(r)) => r,
            Ok(None) | Err(_) => {
                self.power_unavailable = true;
                return Ok(0);
            }
        };
        self.power_unavailable = false;
        self.power_read_ok = true;
        self.power_last = Some(reading);
        let Some(r) = self.power_tracker.decide(reading) else { return Ok(0) };
        let kind = EventKind::PowerStatus { ac: r.ac, charge_percent: r.charge_percent };
        match self.guard.admit_health(mono_ms, utc_ms, kind) {
            Some(ev) => {
                self.commit_health(ev, utc_ms)?;
                Ok(1)
            }
            None => Ok(0),
        }
    }

    /// Lê o inventário (no início e a cada 10 min) e grava só as mudanças. Mesma porta e mesmas travas da saúde: pausa
    /// manual, encerramento e modo restrito param a leitura. Devolve quantos eventos foram gravados.
    fn inventory_tick(&mut self, mono_ms: u64, utc_ms: i64) -> Result<usize, EngineError> {
        if self.inventory.is_none() {
            return Ok(0);
        }
        if !self.settings.inventory_enabled {
            // Desligado pela pessoa: nada é lido e a referência é esquecida, para que ligar de novo comece uma referência
            // nova em vez de gravar como "mudança" o que aconteceu enquanto estava desligado.
            self.inventory_next_mono = None;
            self.inventory_read_ok = false;
            if self.inventory_base != inventory::Baseline::default() {
                self.inventory_base = inventory::Baseline::default();
                if let Some(i) = self.incidents.as_ref() {
                    let _ = i.store.set_setting(INVENTORY_KEY, ""); // melhor esforço
                }
            }
            return Ok(0);
        }
        if !self.guard.health_allowed(mono_ms) {
            self.inventory_next_mono = None; // ao poder ler de novo, lê o estado de AGORA
            return Ok(0);
        }
        if self.inventory_next_mono.is_some_and(|t| mono_ms < t) {
            return Ok(0);
        }
        self.inventory_next_mono = Some(mono_ms.saturating_add(INVENTORY_INTERVAL_MS));
        let Some(src) = self.inventory.as_mut() else { return Ok(0) };
        let snap = match src.read() {
            Ok(s) => s,
            Err(_) => {
                self.inventory_unavailable = true;
                return Ok(0);
            }
        };
        self.inventory_unavailable = false;
        self.inventory_read_ok = true;
        let (changes, next) = inventory::compare(&self.inventory_base, &snap);
        let mut persisted = 0;
        for c in changes {
            let kind = EventKind::InventoryChange { item: c.item, previous: c.previous, current: c.current };
            if let Some(ev) = self.guard.admit_health(mono_ms, utc_ms, kind) {
                self.commit_health(ev, utc_ms)?;
                persisted += 1;
            }
        }
        if next != self.inventory_base {
            if let Some(i) = self.incidents.as_ref() {
                let _ = i.store.set_setting(INVENTORY_KEY, &next.serialize()); // melhor esforço
            }
            self.inventory_base = next;
        }
        Ok(persisted)
    }

    /// Lê e grava os eventos de saúde do ciclo. Devolve quantos foram gravados.
    fn health_tick(&mut self, mono_ms: u64, utc_ms: i64) -> Result<usize, EngineError> {
        if self.health.is_none() {
            return Ok(0);
        }
        if !self.settings.health_log_enabled {
            // Desligado pela pessoa: também descarta o atraso da execução anterior, que não deve ser lido depois.
            if self.health_win.discard() && !self.shutting_down {
                self.save_health_marks(false, None);
            }
            self.health_last_poll_mono = None;
            self.health_read_ok = false;
            return Ok(0);
        }
        if !self.guard.health_allowed(mono_ms) {
            if self.health_win.deactivate() && !self.shutting_down {
                self.save_health_marks(false, None);
            }
            self.health_last_poll_mono = None;
            return Ok(0);
        }
        if self.health_last_poll_mono.is_some_and(|t| mono_ms.saturating_sub(t) < health::POLL_INTERVAL_MS) {
            return Ok(0);
        }
        self.health_last_poll_mono = Some(mono_ms);
        self.health_win.activate(utc_ms);
        let Some(since) = self.health_win.since() else { return Ok(0) };
        let Some(src) = self.health.as_mut() else { return Ok(0) };
        let records = match src.poll(since) {
            Ok(r) => r,
            Err(_) => {
                // Fonte indisponível: segue sem ela, sem erro; a janela fica como está para a próxima tentativa.
                self.health_unavailable = true;
                return Ok(0);
            }
        };
        self.health_unavailable = false;
        self.health_read_ok = true;
        let mut persisted = 0;
        for r in self.health_win.accept(records) {
            let kind = EventKind::HealthEvent { category: r.category, event_id: r.event_id, code: r.code };
            if let Some(ev) = self.guard.admit_health(mono_ms, r.ts_utc_ms, kind) {
                self.commit_health(ev, utc_ms)?;
                persisted += 1;
            }
        }
        self.health_win.finish_poll(utc_ms);
        self.save_health_marks(true, Some(utc_ms));
        Ok(persisted)
    }

    fn tick_inner(&mut self, mono_ms: u64, utc_ms: i64) -> Result<TickReport, EngineError> {
        self.first_seen_utc.get_or_insert(utc_ms);
        // 1. O contexto é sempre observado, com ou sem pausa manual.
        let obs = self.ctx.observe();
        self.guard.observe(mono_ms, obs);

        let (state, reason) = self.guard.state(mono_ms);
        // Janelas posteriores vencem mesmo com a gravação bloqueada.
        self.finalize_captures(utc_ms, false)?;
        // A saúde da máquina não depende do app em primeiro plano: segue mesmo com bloqueio de privacidade.
        // A pausa manual (e o modo restrito) a param no próprio Guard.
        let health_persisted = self.health_tick(mono_ms, utc_ms)? + self.inventory_tick(mono_ms, utc_ms)?
            + self.power_tick(mono_ms, utc_ms)?
            + self.telemetry_tick(mono_ms, utc_ms)?;
        if !state.is_recording() {
            self.stop_recording();
            return Ok(TickReport { state, reason, persisted: health_persisted, dropped_by_guard: 0, incidents_opened: 0 });
        }

        // 2. Só agora as fontes de atividade são consultadas.
        let silent_from = self.gap_start_utc.take();
        if !self.was_recording {
            self.crash_state = CrashState { stretch_start: Some(utc_ms), cursor: utc_ms, seen: Vec::new() };
        }
        self.was_recording = true;
        self.last_recording_utc = Some(utc_ms);

        let samples = self.procs.processes()?;
        // A árvore de processos dos programas excluídos (com a opção dos filhos) vem do instantâneo completo, antes dos
        // eventos do ciclo: nada disto é gravado.
        let refs: Vec<ProcessRef> = samples
            .iter()
            .map(|p| ProcessRef { key: p.key, exe: &p.exe_name, parent_pid: p.parent_pid })
            .collect();
        self.guard.observe_processes(&refs);
        let mut kinds = self.differ.tick(&samples, mono_ms, silent_from);
        let sys = self.procs.system()?;
        kinds.push(EventKind::SystemMetrics {
            cpu_permille: sys.cpu_permille,
            mem_used_kb: sys.mem_used_kb,
            mem_total_kb: sys.mem_total_kb,
        });
        let mut timed: Vec<(i64, EventKind)> = kinds.into_iter().map(|k| (utc_ms, k)).collect();
        timed.extend(self.poll_crashes(utc_ms)?);

        // 3. Todo evento passa pelo Guard antes de chegar ao recorder.
        let (mut persisted, mut dropped) = (health_persisted, 0);
        let mut findings = Vec::new();
        for (ts, kind) in timed {
            match self.guard.admit(mono_ms, ts, kind) {
                Some(ev) => {
                    self.recorder.append(&ev)?;
                    persisted += 1;
                    // O detector só vê eventos já admitidos pelo Guard.
                    if let Some(inc) = self.incidents.as_mut() {
                        findings.extend(inc.detector.observe(ev.kind(), ev.ts_utc_ms()));
                    }
                }
                None => dropped += 1,
            }
        }
        let incidents_opened = findings.len();
        for f in findings {
            self.open_incident(f.kind, f.severity, f.exe_name.map(|e| e.as_str().to_owned()), f.summary, utc_ms)?;
        }
        Ok(TickReport { state, reason, persisted, dropped_by_guard: dropped, incidents_opened })
    }
}
