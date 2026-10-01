//! Engine: compõe collector → Privacy Guard → recorder.
//!
//! Regras:
//! - O contexto é observado a cada ciclo, mesmo com pausa manual (o Guard segue avaliando).
//! - Fontes de atividade só são consultadas enquanto o estado é `Recording`.
//! - Ao parar de gravar, o estado do `Differ` é esquecido; o que ocorrer no
//!   intervalo nunca é reconstruído ao retomar.

pub mod incidents;
pub mod settings;

use std::fmt;
use std::time::Duration;

use bb_collector::{
    CollectError, ContextSource, CrashKind, CrashSource, Differ, MetricsConfig, ProcessSource,
};
use bb_core::{AuthError, EventKind, ExeName, GuardConfig, PrivacyGuard, ReasonCode, RecorderState};
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
}

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
        }
    }

    /// Liga a leitura de falhas e travamentos (Windows Event Log).
    pub fn set_crash_source(&mut self, src: Box<dyn CrashSource + Send>) {
        self.crashes = Some(src);
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
        let Some(inc) = self.incidents.as_ref() else {
            return Err(EngineError::Invalid("store.unavailable".into()));
        };
        let cfg = self.guard.config();
        let rules = bb_query::ExportRules {
            excluded: cfg.excluded_apps.iter().map(|n| n.as_str().to_owned()).collect(),
            protected: cfg.protected_apps.iter().map(|n| n.as_str().to_owned()).collect(),
            authorized_now: self.guard.authorizations(mono_ms).into_iter().map(|a| a.exe.as_str().to_owned()).collect(),
            partial: cfg.partial_exclusions.iter().map(|(n, set)| (n.as_str().to_owned(), *set)).collect(),
        };
        let doc = bb_query::export_incident(&self.recorder, &inc.store, id, &rules, utc_ms)
            .ok_or_else(|| EngineError::Invalid("export.not_found".into()))?;
        std::fs::create_dir_all(out_dir).map_err(|_| EngineError::Invalid("export.folder".into()))?;
        let path = out_dir.join(format!("incident-{id}-{utc_ms}.json"));
        let json = serde_json::to_vec_pretty(&doc).map_err(|_| EngineError::Invalid("export.write".into()))?;
        std::fs::write(&path, json).map_err(|_| EngineError::Invalid("export.write".into()))?;
        Ok(ExportResult { path, events: doc.events.len(), dropped: doc.dropped_events })
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

    fn tick_inner(&mut self, mono_ms: u64, utc_ms: i64) -> Result<TickReport, EngineError> {
        self.first_seen_utc.get_or_insert(utc_ms);
        // 1. O contexto é sempre observado, com ou sem pausa manual.
        let obs = self.ctx.observe();
        self.guard.observe(mono_ms, obs);

        let (state, reason) = self.guard.state(mono_ms);
        // Janelas posteriores vencem mesmo com a gravação bloqueada.
        self.finalize_captures(utc_ms, false)?;
        if !state.is_recording() {
            self.stop_recording();
            return Ok(TickReport { state, reason, persisted: 0, dropped_by_guard: 0, incidents_opened: 0 });
        }

        // 2. Só agora as fontes de atividade são consultadas.
        let silent_from = self.gap_start_utc.take();
        if !self.was_recording {
            self.crash_state = CrashState { stretch_start: Some(utc_ms), cursor: utc_ms, seen: Vec::new() };
        }
        self.was_recording = true;
        self.last_recording_utc = Some(utc_ms);

        let samples = self.procs.processes()?;
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
        let (mut persisted, mut dropped) = (0, 0);
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
            self.open_incident(f.kind, f.severity, Some(f.exe_name.as_str().to_owned()), f.summary, utc_ms)?;
        }
        Ok(TickReport { state, reason, persisted, dropped_by_guard: dropped, incidents_opened })
    }
}
