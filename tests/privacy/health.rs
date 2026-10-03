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

// ---- telemetria de desempenho (PDH) ----

use bb_collector::TelemetrySource;
use bb_core::HealthSample;

#[derive(Default)]
struct Tel {
    sample: HealthSample,
    fail: bool,
    reads: usize,
}
type SharedTel = Arc<Mutex<Tel>>;

struct FakeTel(SharedTel);
impl TelemetrySource for FakeTel {
    fn sample(&mut self) -> Result<HealthSample, CollectError> {
        let mut t = self.0.lock().unwrap();
        t.reads += 1;
        if t.fail {
            return Err(CollectError("synthetic counter failure".into()));
        }
        Ok(t.sample)
    }
}

fn good_sample() -> HealthSample {
    HealthSample {
        thermal_kelvin: Some(318),
        passive_limit_pct: Some(100),
        cpu_load_pct: Some(20),
        cpu_perf_pct: Some(110),
        cpu_freq_mhz: Some(3600),
        mem_commit_pct: Some(55),
        mem_available_mb: Some(9000),
        page_faults_per_sec: Some(700),
        disk_latency_us: Some(900),
        disk_busy_pct: Some(4),
        net_errors: Some(0),
        gpu_pct: Some(6),
    }
}

fn tel_rig() -> (Rig, SharedTel) {
    let mut r = rig();
    let t: SharedTel = Arc::new(Mutex::new(Tel { sample: good_sample(), ..Tel::default() }));
    r.engine.set_telemetry_source(Box::new(FakeTel(t.clone())));
    (r, t)
}

/// 35 s de relógio: passa do intervalo de amostragem (30 s).
fn tel_interval(r: &mut Rig) {
    for _ in 0..7 {
        r.tick();
    }
}

fn sample_lines(r: &mut Rig) -> Vec<String> {
    r.lines().into_iter().filter(|l| l.contains("HealthSample")).collect()
}

fn set_telemetry(r: &mut Rig, on: bool) {
    let mut s = r.engine.settings().clone();
    s.telemetry_enabled = on;
    r.engine.apply_settings(s, r.utc()).unwrap();
}

#[test]
fn a_sample_is_recorded_with_only_the_closed_numeric_fields() {
    let (mut r, t) = tel_rig();
    r.until_recording();
    assert_eq!(t.lock().unwrap().reads, 1, "positive control: one reading, not one per cycle");
    let lines = sample_lines(&mut r);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    let body = v["kind"]["HealthSample"].as_object().expect("HealthSample body");
    let mut keys: Vec<&str> = body.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "cpu_freq_mhz", "cpu_load_pct", "cpu_perf_pct", "disk_busy_pct", "disk_latency_us", "gpu_pct", "mem_available_mb",
            "mem_commit_pct", "net_errors", "page_faults_per_sec", "passive_limit_pct", "thermal_kelvin"
        ]
    );
    assert!(body.values().all(|x| x.is_u64()), "every value is a plain number: {body:?}");
}

#[test]
fn it_samples_about_every_30_seconds_and_not_every_cycle() {
    let (mut r, t) = tel_rig();
    r.until_recording();
    let before = t.lock().unwrap().reads;
    tel_interval(&mut r);
    assert_eq!(t.lock().unwrap().reads, before + 1);
    assert_eq!(sample_lines(&mut r).len(), 2);
}

#[test]
fn the_switch_turns_it_off_at_once_and_back_on() {
    let (mut r, t) = tel_rig();
    r.until_recording();
    set_telemetry(&mut r, false);
    let reads = t.lock().unwrap().reads;
    tel_interval(&mut r);
    tel_interval(&mut r);
    assert_eq!(t.lock().unwrap().reads, reads, "off means the counters are not even read");
    assert_eq!(sample_lines(&mut r).len(), 1, "nothing new while off");
    set_telemetry(&mut r, true);
    r.tick();
    assert_eq!(sample_lines(&mut r).len(), 2, "back on, it samples again right away");
}

#[test]
fn telemetry_is_on_by_default() {
    assert!(bb_engine::Settings::default().telemetry_enabled);
}

#[test]
fn manual_pause_beats_telemetry_and_a_privacy_block_does_not() {
    let (mut r, t) = tel_rig();
    r.until_recording();
    r.engine.pause();
    let reads = t.lock().unwrap().reads;
    tel_interval(&mut r);
    assert_eq!(t.lock().unwrap().reads, reads, "no reading while paused");
    assert_eq!(sample_lines(&mut r).len(), 1);

    r.world.lock().unwrap().foreground = Some("chrome.exe");
    r.engine.resume(r.mono);
    let (state, _) = r.cycle();
    assert_eq!(state, RecorderState::PrivacyBlocked, "setup: really blocked");
    assert!(t.lock().unwrap().reads > reads, "positive control: it samples again");
    assert!(sample_lines(&mut r).len() >= 2, "recorded despite the block (right after the resume, then every 30 s)");
}

#[test]
fn a_missing_counter_source_degrades_without_error() {
    let (mut r, t) = tel_rig();
    r.until_recording();
    t.lock().unwrap().fail = true;
    let (state, _) = r.cycle();
    assert_eq!(state, RecorderState::Recording, "no SafetyFault");
    assert!(r.engine.telemetry_unavailable());
    t.lock().unwrap().fail = false;
    tel_interval(&mut r);
    assert!(!r.engine.telemetry_unavailable());
}

#[test]
fn a_sample_with_no_counter_at_all_is_unavailable_and_not_stored() {
    let (mut r, t) = tel_rig();
    r.until_recording();
    t.lock().unwrap().sample = HealthSample::default();
    tel_interval(&mut r);
    assert!(r.engine.telemetry_unavailable());
    assert_eq!(sample_lines(&mut r).len(), 1, "only the first (good) sample is there");
}

#[test]
fn a_partly_available_sample_keeps_the_missing_ones_as_null() {
    let (mut r, t) = tel_rig();
    t.lock().unwrap().sample = HealthSample { cpu_load_pct: Some(40), ..HealthSample::default() };
    r.until_recording();
    let lines = sample_lines(&mut r);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"cpu_load_pct\":40") && lines[0].contains("\"gpu_pct\":null"), "{}", lines[0]);
}

#[test]
fn sustained_throttling_is_flagged_for_the_incident_logic_and_clears() {
    let (mut r, t) = tel_rig();
    t.lock().unwrap().sample = HealthSample { passive_limit_pct: Some(70), cpu_load_pct: Some(95), ..good_sample() };
    r.until_recording();
    assert!(!r.engine.throttling_sustained(), "one sample is not sustained");
    for _ in 0..12 {
        tel_interval(&mut r);
    }
    assert!(r.engine.throttling_sustained(), "positive control: ~10 samples in a row");
    t.lock().unwrap().sample = good_sample();
    tel_interval(&mut r);
    assert!(!r.engine.throttling_sustained(), "cleared by a normal sample");
}

#[test]
fn turning_telemetry_off_clears_the_throttling_signal() {
    let (mut r, t) = tel_rig();
    t.lock().unwrap().sample = HealthSample { passive_limit_pct: Some(70), cpu_load_pct: Some(95), ..good_sample() };
    r.until_recording();
    for _ in 0..12 {
        tel_interval(&mut r);
    }
    assert!(r.engine.throttling_sustained());
    set_telemetry(&mut r, false);
    r.tick();
    assert!(!r.engine.throttling_sustained());
}

// ---- custo de armazenamento e retenção com amostras sintéticas ----

/// Gerador determinístico simples (xorshift): valores sem padrão, como contadores reais. Valores periódicos comprimem
/// demais e dariam um custo de armazenamento otimista.
struct Rng(u64);
impl Rng {
    fn next(&mut self, max: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % (max + 1)
    }
}

fn sample_events(n: usize) -> Vec<bb_core::ValidatedEvent> {
    let mut g = bb_core::PrivacyGuard::new(GuardConfig::default());
    g.observe(0, Observation { detector_ok: true, session_locked: false, foreground: Some(ExeName::new("synth-editor.exe").unwrap()) });
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    (0..n)
        .map(|i| {
            let s = HealthSample {
                thermal_kelvin: Some(295 + r.next(60) as u16),
                passive_limit_pct: Some(if r.next(20) == 0 { 60 + r.next(39) as u8 } else { 100 }),
                cpu_load_pct: Some(r.next(100) as u8),
                cpu_perf_pct: Some(40 + r.next(120) as u16),
                cpu_freq_mhz: Some(800 + r.next(4200) as u16),
                mem_commit_pct: Some(20 + r.next(75) as u8),
                mem_available_mb: Some(500 + r.next(30_000) as u32),
                page_faults_per_sec: Some(r.next(200_000) as u32),
                disk_latency_us: Some(r.next(50_000) as u32),
                disk_busy_pct: Some(r.next(100) as u8),
                net_errors: Some(r.next(5) as u32),
                gpu_pct: Some(r.next(100) as u8),
            };
            g.admit_health(1, ORIGIN + i as i64 * 30_000, EventKind::HealthSample(s)).expect("health door admits it")
        })
        .collect()
}

use bb_core::EventKind;

#[test]
fn storage_cost_of_a_day_of_samples_is_measured_and_small() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = RecorderConfig { max_events_per_segment: 100_000, max_age: None, ..RecorderConfig::default() };
    let mut rec = Recorder::open(dir.path(), &StaticKey([3u8; 32]), cfg).unwrap();
    const PER_DAY: usize = 2 * 60 * 24; // uma amostra a cada 30 s
    for ev in sample_events(PER_DAY) {
        rec.append(&ev).unwrap();
    }
    // Antes de selar: o journal ativo ainda não está comprimido (pior caso até o segmento ser selado).
    let unsealed = rec.storage_bytes();
    rec.seal().unwrap();
    let bytes = rec.storage_bytes();
    println!(
        "MEASURED: {PER_DAY} samples/day: journal not yet sealed = {unsealed} bytes ({} bytes/sample); sealed = {bytes} bytes/day = {:.3} MB/day ({} bytes/sample)",
        unsealed / PER_DAY as u64,
        bytes as f64 / 1e6,
        bytes / PER_DAY as u64
    );
    assert!(bytes > 0);
    assert!(bytes < 2_000_000, "a day of 30 s samples must stay under 2 MB: {bytes}");
}

#[test]
fn retention_keeps_the_samples_within_the_budget_and_drops_the_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let events = sample_events(6_000); // ~2 dias de amostras
    // mede quanto as 6000 amostras ocupam sem limite e usa um terço como orçamento
    let probe_dir = tempfile::tempdir().unwrap();
    let probe_cfg = RecorderConfig { max_events_per_segment: 200, max_age: None, ..RecorderConfig::default() };
    let mut probe = Recorder::open(probe_dir.path(), &StaticKey([3u8; 32]), probe_cfg).unwrap();
    for ev in &events {
        probe.append(ev).unwrap();
    }
    probe.seal().unwrap();
    let budget = probe.storage_bytes() / 3;
    assert!(budget > 10_000, "setup: a meaningful budget ({budget})");
    let cfg = RecorderConfig { max_events_per_segment: 200, max_total_bytes: budget, max_age: None, ..RecorderConfig::default() };
    let mut rec = Recorder::open(dir.path(), &StaticKey([3u8; 32]), cfg).unwrap();
    for ev in &events {
        rec.append(ev).unwrap();
    }
    rec.seal().unwrap();
    rec.enforce_retention().unwrap();
    assert!(rec.storage_bytes() <= budget, "over budget: {} > {budget}", rec.storage_bytes());
    let lines: Vec<String> = rec.list_segments().unwrap().iter().flat_map(|s| rec.read_segment(s.index).unwrap()).collect();
    assert!(!lines.is_empty(), "something must remain");
    assert!(lines.len() < 6_000, "positive control: the oldest samples were actually removed");
    let ts: Vec<i64> = lines.iter().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["ts_utc_ms"].as_i64().unwrap()).collect();
    assert!(ts.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(*ts.last().unwrap(), ORIGIN + 5_999 * 30_000, "the newest sample is always kept");
    assert!(ts[0] > ORIGIN, "the oldest are gone");
}
