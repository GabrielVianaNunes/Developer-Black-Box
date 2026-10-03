//! Testes de privacidade dos eventos de saúde da máquina: motor + Guard + gravador, com fontes falsas e dados sintéticos.
//!
//! Regras sob teste: só o Guard cria o evento; a pausa manual sempre vence; um bloqueio de privacidade NÃO impede;
//! o que ocorreu numa pausa nunca é gravado depois; fonte indisponível degrada sem erro; nada de texto livre.

use std::sync::{Arc, Mutex};

use bb_collector::{
    CollectError, ContextSource, HealthRecord, HealthSource, MetricsConfig, ProcessSample, ProcessSource, SystemSample,
};
use bb_core::{ExeName, GuardConfig, HealthCategory, Observation, RecorderState};
use bb_engine::{Engine, IncidentConfig};
use bb_recorder::{Recorder, RecorderConfig, StaticKey};
use bb_store::Store;

#[derive(Default)]
struct World {
    foreground: Option<&'static str>,
}
type Shared = Arc<Mutex<World>>;

struct NoProcs;
impl ProcessSource for NoProcs {
    fn processes(&mut self) -> Result<Vec<ProcessSample>, CollectError> {
        Ok(Vec::new())
    }
    fn system(&mut self) -> Result<SystemSample, CollectError> {
        Ok(SystemSample { cpu_permille: 1, mem_used_kb: 1, mem_total_kb: 2 })
    }
}

struct Ctx(Shared);
impl ContextSource for Ctx {
    fn observe(&mut self) -> Observation {
        Observation {
            detector_ok: true,
            session_locked: false,
            foreground: self.0.lock().unwrap().foreground.map(|n| ExeName::new(n).unwrap()),
        }
    }
}

#[derive(Default)]
struct Feed {
    records: Vec<HealthRecord>,
    fail: bool,
    /// O `since` de cada consulta recebida.
    polls: Vec<i64>,
}
type SharedFeed = Arc<Mutex<Feed>>;

struct FakeHealth(SharedFeed);
impl HealthSource for FakeHealth {
    fn poll(&mut self, since: i64) -> Result<Vec<HealthRecord>, CollectError> {
        let mut f = self.0.lock().unwrap();
        f.polls.push(since);
        if f.fail {
            return Err(CollectError("synthetic channel failure".into()));
        }
        // uma fonte "ingênua": devolve tudo a partir de `since`, sem saber de pausas
        Ok(f.records.iter().copied().filter(|r| r.ts_utc_ms >= since).collect())
    }
}

fn rec(ts: i64, cat: HealthCategory, id: u16, code: Option<u32>) -> HealthRecord {
    HealthRecord { ts_utc_ms: ts, category: cat, event_id: id, code }
}

const ORIGIN: i64 = 1_000_000_000;
/// Menor que a validade da observação do Guard (10 s); sete ciclos passam do intervalo de consulta (30 s).
const STEP: u64 = 5_000;
const CYCLE: usize = 7;

struct Rig {
    engine: Engine<NoProcs, Ctx>,
    world: Shared,
    feed: SharedFeed,
    mono: u64,
    _dir: tempfile::TempDir,
}

fn cfg() -> GuardConfig {
    GuardConfig { stability_window_ms: 1_000, ..GuardConfig::default() }
}

fn engine_on(dir: &std::path::Path, world: &Shared, store: Option<Store>) -> Engine<NoProcs, Ctx> {
    let rcfg = RecorderConfig { max_events_per_segment: 10_000, max_age: None, ..RecorderConfig::default() };
    let recorder = Recorder::open(dir, &StaticKey([3u8; 32]), rcfg).unwrap();
    let metrics = MetricsConfig { every_n_ticks: 1, min_cpu_permille: 0, min_working_set_kb: 0 };
    let mut e = Engine::new(cfg(), metrics, recorder, NoProcs, Ctx(world.clone()), 1);
    if let Some(s) = store {
        e.enable_incidents(s, IncidentConfig::default());
    }
    e
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let world: Shared = Arc::new(Mutex::new(World { foreground: Some("synth-editor.exe") }));
    let feed: SharedFeed = Arc::default();
    let mut engine = engine_on(dir.path(), &world, Some(Store::open_in_memory().unwrap()));
    engine.set_health_source(Box::new(FakeHealth(feed.clone())), ORIGIN);
    Rig { engine, world, feed, mono: 0, _dir: dir }
}

impl Rig {
    fn utc(&self) -> i64 {
        ORIGIN + self.mono as i64
    }
    fn tick(&mut self) -> bb_engine::TickReport {
        self.mono += STEP;
        self.engine.tick(self.mono, self.utc()).unwrap()
    }
    /// Passa do intervalo de consulta de saúde. Devolve o estado final e quantos eventos foram gravados no total.
    fn cycle(&mut self) -> (RecorderState, usize) {
        let mut total = 0;
        let mut state = RecorderState::Starting;
        for _ in 0..CYCLE {
            let rep = self.tick();
            state = rep.state;
            total += rep.persisted;
        }
        (state, total)
    }
    fn until_recording(&mut self) {
        for _ in 0..6 {
            if self.tick().state == RecorderState::Recording {
                return;
            }
        }
        panic!("never reached Recording");
    }
    fn lines(&mut self) -> Vec<String> {
        self.engine.recorder_mut().seal().unwrap();
        let rec = self.engine.recorder();
        rec.list_segments().unwrap().iter().flat_map(|s| rec.read_segment(s.index).unwrap()).collect()
    }
    fn health_lines(&mut self) -> Vec<String> {
        self.lines().into_iter().filter(|l| l.contains("HealthEvent")).collect()
    }
    fn polls(&self) -> usize {
        self.feed.lock().unwrap().polls.len()
    }
    fn push(&self, r: HealthRecord) {
        self.feed.lock().unwrap().records.push(r);
    }
}

#[test]
fn health_events_are_recorded_with_only_category_id_and_number() {
    let mut r = rig();
    r.until_recording();
    let ts = r.utc() + 5;
    r.push(rec(ts, HealthCategory::BugCheck, 1001, Some(0xd1)));
    r.cycle();
    let lines = r.health_lines();
    assert_eq!(lines.len(), 1, "positive control: the event must be stored: {lines:?}");
    assert!(lines[0].contains("\"BugCheck\"") && lines[0].contains("1001") && lines[0].contains("209"));
    // o corpo gravado tem exatamente estas chaves: nenhum texto livre
    let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    let body = v["kind"]["HealthEvent"].as_object().expect("HealthEvent body");
    let mut keys: Vec<&str> = body.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["category", "code", "event_id"]);
}

#[test]
fn a_privacy_block_does_not_stop_health_but_still_stops_activity() {
    let mut r = rig();
    r.until_recording();
    // um app protegido vai para o primeiro plano: a atividade para, a saúde não
    r.world.lock().unwrap().foreground = Some("chrome.exe");
    let (state, _) = r.cycle();
    assert_eq!(state, RecorderState::PrivacyBlocked, "setup: really blocked");
    let activity_before = r.lines().iter().filter(|l| l.contains("SystemMetrics")).count();
    let ts = r.utc() + 5;
    r.push(rec(ts, HealthCategory::DiskError, 51, None));
    let (state, persisted) = r.cycle();
    assert_eq!(state, RecorderState::PrivacyBlocked);
    assert_eq!(persisted, 1, "only the health event is persisted");
    assert_eq!(r.health_lines().len(), 1);
    let activity_after = r.lines().iter().filter(|l| l.contains("SystemMetrics")).count();
    assert_eq!(activity_before, activity_after, "no activity while blocked");
}

#[test]
fn manual_pause_beats_health_and_nothing_from_the_pause_is_recorded_after() {
    let mut r = rig();
    r.until_recording();
    r.engine.pause();
    let polls = r.polls();
    let in_pause = r.utc() + 10;
    r.push(rec(in_pause, HealthCategory::HardwareError, 18, None));
    for _ in 0..3 {
        let rep = r.tick();
        assert_eq!((rep.state, rep.persisted), (RecorderState::ManualPause, 0));
    }
    assert_eq!(r.polls(), polls, "the Event Log must not even be queried while paused");

    // retomar: a leitura volta, mas o evento da pausa nunca entra
    r.engine.resume(r.mono);
    r.cycle();
    r.cycle();
    assert!(r.polls() > polls, "positive control: reading resumed");
    let after = r.utc() + 10;
    r.push(rec(after, HealthCategory::HardwareError, 19, None));
    r.cycle();
    let lines = r.health_lines();
    assert!(lines.iter().any(|l| l.contains("19")), "the event after the resume is recorded: {lines:?}");
    assert!(!lines.iter().any(|l| l.contains("\"event_id\":18")), "the pause event must never be recorded: {lines:?}");
}

#[test]
fn an_unavailable_source_degrades_without_error_and_recovers() {
    let mut r = rig();
    r.until_recording();
    r.feed.lock().unwrap().fail = true;
    let (state, _) = r.cycle(); // não pode virar erro nem SafetyFault
    assert_eq!(state, RecorderState::Recording);
    assert!(r.engine.health_unavailable());
    r.feed.lock().unwrap().fail = false;
    let ts = r.utc() + 5;
    r.push(rec(ts, HealthCategory::DisplayDriverReset, 4101, None));
    r.cycle();
    assert!(!r.engine.health_unavailable());
    assert_eq!(r.health_lines().len(), 1, "the event that happened while the channel was down is read once it is back");
}

#[test]
fn the_source_is_polled_at_most_once_per_interval() {
    let mut r = rig();
    r.until_recording();
    let before = r.polls();
    // ciclos de 2 s por 20 s: nenhuma consulta nova
    for _ in 0..10 {
        r.mono += 2_000;
        r.engine.tick(r.mono, r.utc()).unwrap();
    }
    assert_eq!(r.polls(), before);
}

#[test]
fn a_burst_of_identical_disk_errors_is_stored_once() {
    let mut r = rig();
    r.until_recording();
    let t0 = r.utc();
    for i in 0..40 {
        r.push(rec(t0 + 5 + i * 50, HealthCategory::DiskError, 51, None));
    }
    r.cycle();
    assert_eq!(r.health_lines().len(), 1);
}

#[test]
fn a_restart_reads_what_happened_while_the_app_was_closed_but_not_what_happened_in_the_new_pause() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("meta.db");
    let world: Shared = Arc::new(Mutex::new(World { foreground: Some("synth-editor.exe") }));
    let feed: SharedFeed = Arc::default();

    // execução 1: lendo, depois encerra normalmente
    {
        let mut e = engine_on(&dir.path().join("rec"), &world, Some(Store::open(&db).unwrap()));
        e.set_health_source(Box::new(FakeHealth(feed.clone())), ORIGIN);
        let mut mono = 0u64;
        for _ in 0..4 {
            mono += STEP;
            e.tick(mono, ORIGIN + mono as i64).unwrap();
        }
        e.shutdown().unwrap();
        // um ciclo depois de encerrar NÃO pode contar como pausa (a marca d'água segue "lendo")
        mono += STEP;
        e.tick(mono, ORIGIN + mono as i64).unwrap();
    }
    let closed_at = ORIGIN + 4 * STEP as i64;
    let restart = closed_at + 600_000; // o app ficou fechado 10 min
    // o Windows registra o desligamento inesperado no boot seguinte (dentro do intervalo fechado)
    feed.lock().unwrap().records.push(rec(closed_at + 30_000, HealthCategory::UnexpectedShutdown, 41, Some(0)));
    // e há um evento que acontecerá na pausa desta nova execução
    let during_pause = restart + 60_000;
    feed.lock().unwrap().records.push(rec(during_pause, HealthCategory::DiskError, 7, None));

    // execução 2: abre PAUSADA (padrão conservador), o usuário retoma bem depois
    let mut e = engine_on(&dir.path().join("rec"), &world, Some(Store::open(&db).unwrap()));
    e.set_health_source(Box::new(FakeHealth(feed.clone())), restart);
    e.pause();
    let mut mono = 0u64;
    for _ in 0..20 {
        // 100 s de pausa; o evento "da pausa" está em restart + 60 s
        mono += STEP;
        let rep = e.tick(mono, restart + mono as i64).unwrap();
        assert_eq!(rep.persisted, 0, "paused: nothing is read");
    }
    e.resume(mono);
    for _ in 0..8 {
        mono += STEP;
        e.tick(mono, restart + mono as i64).unwrap();
    }
    e.recorder_mut().seal().unwrap();
    let rec_ = e.recorder();
    let lines: Vec<String> = rec_.list_segments().unwrap().iter().flat_map(|s| rec_.read_segment(s.index).unwrap()).collect();
    let health: Vec<&String> = lines.iter().filter(|l| l.contains("HealthEvent")).collect();
    assert!(health.iter().any(|l| l.contains("UnexpectedShutdown")), "the shutdown from the closed interval is read: {health:?}");
    assert!(!health.iter().any(|l| l.contains("DiskError")), "the new pause is never reconstructed: {health:?}");
}

#[test]
fn quitting_while_paused_leaves_no_backlog_for_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("meta.db");
    let world: Shared = Arc::new(Mutex::new(World { foreground: Some("synth-editor.exe") }));
    let feed: SharedFeed = Arc::default();
    {
        let mut e = engine_on(&dir.path().join("rec"), &world, Some(Store::open(&db).unwrap()));
        e.set_health_source(Box::new(FakeHealth(feed.clone())), ORIGIN);
        let mut mono = 0u64;
        for _ in 0..4 {
            mono += STEP;
            e.tick(mono, ORIGIN + mono as i64).unwrap();
        }
        e.pause();
        mono += STEP;
        e.tick(mono, ORIGIN + mono as i64).unwrap(); // o ciclo percebe a pausa e marca "parou de ler"
        e.shutdown().unwrap();
    }
    let restart = ORIGIN + 10 * STEP as i64;
    feed.lock().unwrap().records.push(rec(ORIGIN + 6 * STEP as i64, HealthCategory::UnexpectedShutdown, 6008, None));
    let mut e = engine_on(&dir.path().join("rec"), &world, Some(Store::open(&db).unwrap()));
    e.set_health_source(Box::new(FakeHealth(feed.clone())), restart);
    let mut mono = 0u64;
    for _ in 0..8 {
        mono += STEP;
        e.tick(mono, restart + mono as i64).unwrap();
    }
    assert!(!feed.lock().unwrap().polls.is_empty(), "positive control: it did read");
    e.recorder_mut().seal().unwrap();
    let rec_ = e.recorder();
    let stored = rec_.list_segments().unwrap().iter().flat_map(|s| rec_.read_segment(s.index).unwrap()).filter(|l| l.contains("HealthEvent")).count();
    assert_eq!(stored, 0, "events from a stretch that ended in a manual pause are never reconstructed");
}

// ---- inventário: só mudanças, só números ----

use bb_collector::{InventorySource, Snapshot};
use bb_core::InventoryItem;

#[derive(Default)]
struct Inv {
    snap: Vec<(InventoryItem, Option<u64>)>,
    fail: bool,
    reads: usize,
}
type SharedInv = Arc<Mutex<Inv>>;

struct FakeInv(SharedInv);
impl InventorySource for FakeInv {
    fn read(&mut self) -> Result<Snapshot, CollectError> {
        let mut i = self.0.lock().unwrap();
        i.reads += 1;
        if i.fail {
            return Err(CollectError("synthetic inventory failure".into()));
        }
        Ok(Snapshot(i.snap.clone()))
    }
}

fn inv_rig() -> (Rig, SharedInv) {
    let mut r = rig();
    let inv: SharedInv = Arc::new(Mutex::new(Inv {
        snap: vec![(InventoryItem::OsBuild, Some(100)), (InventoryItem::SecureBoot, Some(1))],
        ..Inv::default()
    }));
    r.engine.set_inventory_source(Box::new(FakeInv(inv.clone())));
    (r, inv)
}

/// 10 min de relógio monotônico: passa do intervalo de leitura do inventário.
fn inventory_interval(r: &mut Rig) {
    for _ in 0..125 {
        r.tick();
    }
}

fn inventory_lines(r: &mut Rig) -> Vec<String> {
    r.lines().into_iter().filter(|l| l.contains("InventoryChange")).collect()
}

#[test]
fn the_first_inventory_reading_is_a_baseline_and_a_later_change_is_recorded_with_previous_and_new() {
    let (mut r, inv) = inv_rig();
    r.until_recording();
    assert_eq!(inv.lock().unwrap().reads, 1, "positive control: it read once, and not again on every cycle");
    assert!(inventory_lines(&mut r).is_empty(), "the first reading is only a baseline");

    inv.lock().unwrap().snap = vec![(InventoryItem::OsBuild, Some(101)), (InventoryItem::SecureBoot, Some(1))];
    inventory_interval(&mut r);
    assert_eq!(inv.lock().unwrap().reads, 2, "one more reading after the 10-minute interval");
    let lines = inventory_lines(&mut r);
    assert_eq!(lines.len(), 1, "only the item that changed: {lines:?}");
    let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    let body = &v["kind"]["InventoryChange"];
    assert_eq!((body["item"].as_str(), body["previous"].as_u64(), body["current"].as_u64()), (Some("OsBuild"), Some(100), Some(101)));
    let mut keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["current", "item", "previous"], "numbers and an item name, nothing else");

    // a mesma leitura de novo não repete o evento
    inventory_interval(&mut r);
    assert_eq!(inventory_lines(&mut r).len(), 1);
}

#[test]
fn an_unavailable_inventory_is_not_a_change() {
    let (mut r, inv) = inv_rig();
    r.until_recording();
    inv.lock().unwrap().fail = true;
    inventory_interval(&mut r);
    assert!(r.engine.inventory_unavailable());
    inv.lock().unwrap().fail = false;
    inventory_interval(&mut r);
    assert!(!r.engine.inventory_unavailable());
    assert!(inventory_lines(&mut r).is_empty(), "failing to read and reading the same values again is no change");
}

#[test]
fn inventory_follows_the_same_gates_as_health_pause_wins_and_a_block_does_not_stop_it() {
    let (mut r, inv) = inv_rig();
    r.until_recording();
    // pausa manual: nem lê
    r.engine.pause();
    let reads = inv.lock().unwrap().reads;
    inv.lock().unwrap().snap = vec![(InventoryItem::OsBuild, Some(200))];
    inventory_interval(&mut r);
    assert_eq!(inv.lock().unwrap().reads, reads, "no reading while paused");
    assert!(inventory_lines(&mut r).is_empty());

    // retoma sob um bloqueio de privacidade: o inventário é lido e gravado mesmo assim
    r.world.lock().unwrap().foreground = Some("chrome.exe");
    r.engine.resume(r.mono);
    let (state, _) = r.cycle();
    assert_eq!(state, RecorderState::PrivacyBlocked, "setup: really blocked");
    assert!(inv.lock().unwrap().reads > reads, "positive control: read again after the resume");
    assert_eq!(inventory_lines(&mut r).len(), 1, "the change is recorded despite the block");
}

#[test]
fn the_baseline_survives_a_restart_so_a_change_made_while_closed_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("meta.db");
    let world: Shared = Arc::new(Mutex::new(World { foreground: Some("synth-editor.exe") }));
    let inv: SharedInv = Arc::new(Mutex::new(Inv { snap: vec![(InventoryItem::BiosVersion, Some(7))], ..Inv::default() }));
    let run = |inv: &SharedInv| -> Vec<String> {
        let mut e = engine_on(&dir.path().join("rec"), &world, Some(Store::open(&db).unwrap()));
        e.set_inventory_source(Box::new(FakeInv(inv.clone())));
        let mut mono = 0u64;
        for _ in 0..8 {
            mono += STEP;
            e.tick(mono, ORIGIN + mono as i64).unwrap();
        }
        e.shutdown().unwrap();
        let rec_ = e.recorder();
        rec_.list_segments().unwrap().iter().flat_map(|s| rec_.read_segment(s.index).unwrap()).filter(|l| l.contains("InventoryChange")).collect()
    };
    assert!(run(&inv).is_empty(), "first run: baseline only");
    inv.lock().unwrap().snap = vec![(InventoryItem::BiosVersion, Some(8))];
    let lines = run(&inv);
    assert_eq!(lines.len(), 1, "second run sees the update: {lines:?}");
    assert!(lines[0].contains("BiosVersion") && lines[0].contains('7') && lines[0].contains('8'));
}

#[test]
fn the_stored_baseline_contains_only_known_keys_and_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("meta.db");
    let world: Shared = Arc::new(Mutex::new(World { foreground: Some("synth-editor.exe") }));
    let inv: SharedInv = Arc::new(Mutex::new(Inv {
        snap: vec![(InventoryItem::BiosVersion, Some(5)), (InventoryItem::DeviceProblemCodes, Some(1 << 10))],
        ..Inv::default()
    }));
    let mut e = engine_on(&dir.path().join("rec"), &world, Some(Store::open(&db).unwrap()));
    e.set_inventory_source(Box::new(FakeInv(inv)));
    for i in 1..=4u64 {
        e.tick(i * STEP, ORIGIN + (i * STEP) as i64).unwrap();
    }
    let stored = Store::open(&db).unwrap().get_setting("health.inventory").unwrap().expect("baseline saved");
    assert_eq!(stored, "bios_version=5;problem_codes=1024");
}

// ---- energia e bateria ----

use bb_collector::{PowerReading, PowerSource};
use bb_core::AcLine;

#[derive(Default)]
struct Pwr {
    reading: Option<PowerReading>,
    fail: bool,
    reads: usize,
}
type SharedPwr = Arc<Mutex<Pwr>>;

struct FakePower(SharedPwr);
impl PowerSource for FakePower {
    fn read(&mut self) -> Result<Option<PowerReading>, CollectError> {
        let mut p = self.0.lock().unwrap();
        p.reads += 1;
        if p.fail {
            return Err(CollectError("synthetic power failure".into()));
        }
        Ok(p.reading)
    }
}

fn battery(ac: AcLine, pct: u8) -> Option<PowerReading> {
    Some(PowerReading { ac: Some(ac), charge_percent: Some(pct) })
}

fn power_rig(reading: Option<PowerReading>) -> (Rig, SharedPwr) {
    let mut r = rig();
    let p: SharedPwr = Arc::new(Mutex::new(Pwr { reading, ..Pwr::default() }));
    r.engine.set_power_source(Box::new(FakePower(p.clone())));
    (r, p)
}

/// 70 s de relógio: passa do intervalo de leitura de energia (60 s).
fn power_interval(r: &mut Rig) {
    for _ in 0..14 {
        r.tick();
    }
}

fn power_lines(r: &mut Rig) -> Vec<String> {
    r.lines().into_iter().filter(|l| l.contains("PowerStatus")).collect()
}

#[test]
fn the_first_power_reading_is_recorded_with_only_ac_and_percent() {
    let (mut r, p) = power_rig(battery(AcLine::Offline, 80));
    r.until_recording();
    assert_eq!(p.lock().unwrap().reads, 1, "read once, and not again on every cycle");
    let lines = power_lines(&mut r);
    assert_eq!(lines.len(), 1, "positive control: {lines:?}");
    let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    let body = v["kind"]["PowerStatus"].as_object().expect("PowerStatus body");
    let mut keys: Vec<&str> = body.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["ac", "charge_percent"]);
    assert_eq!((body["ac"].as_str(), body["charge_percent"].as_u64()), (Some("Offline"), Some(80)));
}

#[test]
fn only_meaningful_changes_are_recorded() {
    let (mut r, p) = power_rig(battery(AcLine::Offline, 80));
    r.until_recording();
    p.lock().unwrap().reading = battery(AcLine::Offline, 78);
    power_interval(&mut r);
    assert_eq!(power_lines(&mut r).len(), 1, "a 2-point drop is not worth a record");
    p.lock().unwrap().reading = battery(AcLine::Online, 78);
    power_interval(&mut r);
    assert_eq!(power_lines(&mut r).len(), 2, "plugging in is");
    p.lock().unwrap().reading = battery(AcLine::Online, 90);
    power_interval(&mut r);
    assert_eq!(power_lines(&mut r).len(), 3, "a 12-point charge step is");
}

#[test]
fn a_desktop_without_a_battery_is_unavailable_and_records_nothing() {
    let (mut r, p) = power_rig(None);
    r.until_recording();
    power_interval(&mut r);
    assert!(p.lock().unwrap().reads >= 1, "positive control: it did try to read");
    assert!(r.engine.power_unavailable());
    assert!(power_lines(&mut r).is_empty());
}

#[test]
fn a_failing_power_source_degrades_without_error_and_recovers() {
    let (mut r, p) = power_rig(battery(AcLine::Online, 100));
    r.until_recording();
    p.lock().unwrap().fail = true;
    p.lock().unwrap().reading = battery(AcLine::Offline, 40);
    power_interval(&mut r);
    assert!(r.engine.power_unavailable());
    p.lock().unwrap().fail = false;
    power_interval(&mut r);
    assert!(!r.engine.power_unavailable());
    assert_eq!(power_lines(&mut r).len(), 2, "first reading plus the change seen after the recovery");
}

#[test]
fn power_obeys_the_pause_and_survives_a_privacy_block() {
    let (mut r, p) = power_rig(battery(AcLine::Offline, 80));
    r.until_recording();
    r.engine.pause();
    let reads = p.lock().unwrap().reads;
    p.lock().unwrap().reading = battery(AcLine::Online, 30);
    power_interval(&mut r);
    assert_eq!(p.lock().unwrap().reads, reads, "no reading while paused");
    assert_eq!(power_lines(&mut r).len(), 1, "nothing new from the pause");

    r.world.lock().unwrap().foreground = Some("chrome.exe");
    r.engine.resume(r.mono);
    let (state, _) = r.cycle();
    assert_eq!(state, RecorderState::PrivacyBlocked, "setup: really blocked");
    let lines = power_lines(&mut r);
    assert_eq!(lines.len(), 2, "after the resume the current state is recorded again, even under a block: {lines:?}");
    assert!(lines[1].contains("\"Online\"") && lines[1].contains("30"));
}

#[test]
fn sleep_and_resume_come_from_the_event_log_with_the_same_gates() {
    let mut r = rig();
    r.until_recording();
    let ts = r.utc() + 5;
    r.push(rec(ts, HealthCategory::SleepEntered, 42, Some(3)));
    r.push(rec(ts + 20_000, HealthCategory::Resumed, 107, None));
    r.cycle();
    let lines = r.health_lines();
    assert!(lines.iter().any(|l| l.contains("SleepEntered") && l.contains('3')), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("Resumed")), "{lines:?}");
}

#[test]
fn after_a_pause_the_current_power_state_is_recorded_again_even_if_it_did_not_change() {
    let (mut r, _p) = power_rig(battery(AcLine::Offline, 80));
    r.until_recording();
    assert_eq!(power_lines(&mut r).len(), 1);
    r.engine.pause();
    power_interval(&mut r);
    r.engine.resume(r.mono);
    r.cycle();
    assert_eq!(power_lines(&mut r).len(), 2, "the state after a pause is a fresh reference, so it is recorded");
}

#[test]
fn power_is_read_once_per_interval_not_on_every_cycle() {
    let (mut r, p) = power_rig(battery(AcLine::Offline, 80));
    r.until_recording();
    let before = p.lock().unwrap().reads;
    power_interval(&mut r);
    assert_eq!(p.lock().unwrap().reads, before + 1, "one more reading after 70 s");
}
