//! O esquema fechado dos eventos de saúde: percorre TODAS as variantes e confere que o que é gravado são só números,
//! nulos e os nomes de enumerações fechadas; nenhum texto livre, nenhum identificador. Se alguém acrescentar uma variante
//! (categoria, item, tomada) ou um tipo de evento de saúde, este arquivo deixa de compilar até a lista ser atualizada,
//! e o teste então exige que o novo valor também respeite as regras.

use std::collections::BTreeSet;

use bb_core::{
    AcLine, EventKind, ExeName, GuardConfig, HealthCategory, HealthSample, InventoryItem, Observation, PrivacyGuard, ValidatedEvent,
};
use serde_json::Value;

/// Lista de todas as variantes. O `match` sem coringa obriga a atualizar a lista quando surge uma nova.
fn categories() -> Vec<HealthCategory> {
    use HealthCategory::*;
    let all = vec![
        UnexpectedShutdown,
        BugCheck,
        HardwareError,
        DisplayDriverReset,
        DiskError,
        FileSystemError,
        ServiceCrash,
        UpdateFailure,
        SleepEntered,
        Resumed,
    ];
    for c in &all {
        match c {
            UnexpectedShutdown | BugCheck | HardwareError | DisplayDriverReset | DiskError | FileSystemError | ServiceCrash
            | UpdateFailure | SleepEntered | Resumed => {}
        }
    }
    all
}

fn items() -> Vec<InventoryItem> {
    use InventoryItem::*;
    let all = vec![BiosVersion, BiosDate, FirmwareType, SecureBoot, OsBuild, DeviceProblemCount, DeviceProblemCodes];
    for i in &all {
        match i {
            BiosVersion | BiosDate | FirmwareType | SecureBoot | OsBuild | DeviceProblemCount | DeviceProblemCodes => {}
        }
    }
    all
}

fn acs() -> Vec<AcLine> {
    let all = vec![AcLine::Offline, AcLine::Online];
    for a in &all {
        match a {
            AcLine::Offline | AcLine::Online => {}
        }
    }
    all
}

/// Todos os tipos de evento de saúde, em todas as combinações de enumeração. O `match` sobre `EventKind` não tem coringa:
/// um novo tipo de evento obriga a decidir aqui se é de saúde.
fn health_kinds() -> Vec<EventKind> {
    let mut v = Vec::new();
    for c in categories() {
        v.push(EventKind::HealthEvent { category: c, event_id: 1001, code: Some(0xd1) });
        v.push(EventKind::HealthEvent { category: c, event_id: 41, code: None });
    }
    for i in items() {
        v.push(EventKind::InventoryChange { item: i, previous: Some(1), current: Some(2) });
        v.push(EventKind::InventoryChange { item: i, previous: None, current: None });
    }
    for a in acs() {
        v.push(EventKind::PowerStatus { ac: Some(a), charge_percent: Some(80) });
    }
    v.push(EventKind::PowerStatus { ac: None, charge_percent: None });
    v.push(EventKind::HealthSample(full_sample()));
    v.push(EventKind::HealthSample(HealthSample::default()));
    for k in &v {
        match k {
            EventKind::HealthEvent { .. }
            | EventKind::InventoryChange { .. }
            | EventKind::PowerStatus { .. }
            | EventKind::HealthSample(_) => {}
            // os tipos que NÃO são de saúde ficam fora desta lista, mas precisam ser nomeados aqui
            EventKind::ProcessStarted { .. }
            | EventKind::ProcessExited { .. }
            | EventKind::ProcessMetrics { .. }
            | EventKind::SystemMetrics { .. }
            | EventKind::AppCrash { .. }
            | EventKind::AppHang { .. }
            | EventKind::RecorderStateChanged { .. }
            | EventKind::UserMarker { .. } => unreachable!("not a health kind"),
        }
    }
    v
}

fn full_sample() -> HealthSample {
    HealthSample {
        thermal_kelvin: Some(318),
        passive_limit_pct: Some(100),
        cpu_load_pct: Some(23),
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

fn admitted(kind: EventKind) -> ValidatedEvent {
    let mut g = PrivacyGuard::new(GuardConfig::default());
    g.observe(0, Observation { detector_ok: true, session_locked: false, foreground: Some(ExeName::new("synth-editor.exe").unwrap()) });
    g.admit_health(1, 1_000, kind).expect("the health door admits every health kind")
}

/// Os únicos textos que podem aparecer: nomes de enumerações fechadas e os nomes fixos das chaves.
fn closed_names() -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    s.extend(categories().iter().map(|c| format!("{c:?}")));
    s.extend(items().iter().map(|i| format!("{i:?}")));
    s.extend(acs().iter().map(|a| format!("{a:?}")));
    s
}

/// Percorre o JSON: toda folha é número, nulo ou um nome fechado; toda chave de objeto é um identificador curto fixo.
fn check_leaves(v: &Value, path: &str, allowed: &BTreeSet<String>, problems: &mut Vec<String>) {
    match v {
        Value::Null | Value::Number(_) => {}
        Value::String(s) if allowed.contains(s) => {}
        Value::String(s) => problems.push(format!("{path}: free text {s:?}")),
        Value::Bool(_) => problems.push(format!("{path}: unexpected bool")),
        Value::Array(_) => problems.push(format!("{path}: unexpected array")),
        Value::Object(o) => {
            for (k, x) in o {
                assert!(k.len() <= 24 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "odd key {k:?} at {path}");
                check_leaves(x, &format!("{path}.{k}"), allowed, problems);
            }
        }
    }
}

#[test]
fn every_health_kind_is_stored_as_numbers_nulls_and_closed_names_only() {
    let allowed = closed_names();
    let kinds = health_kinds();
    let expected = categories().len() * 2 + items().len() * 2 + acs().len() + 1 + 2;
    assert_eq!(kinds.len(), expected, "positive control: the walk really covers every variant");
    for k in kinds {
        let json = serde_json::to_value(admitted(k.clone())).unwrap();
        let mut problems = Vec::new();
        check_leaves(&json, "$", &allowed, &mut problems);
        assert!(problems.is_empty(), "{k:?} -> {problems:?}");
    }
}

#[test]
fn the_text_check_would_catch_free_text_and_identifiers() {
    // controle negativo do próprio teste: o verificador precisa reprovar o que não pode existir
    let allowed = closed_names();
    for bad in [
        serde_json::json!({"note": "C:\\Users\\synth-user\\a.txt"}),
        serde_json::json!({"category": "SynthServiceName"}),
        serde_json::json!({"serial": "SYNTH-SERIAL-0001"}),
        serde_json::json!({"ssid": ["synth-wifi"]}),
        serde_json::json!({"flag": true}),
    ] {
        let mut problems = Vec::new();
        check_leaves(&bad, "$", &allowed, &mut problems);
        assert!(!problems.is_empty(), "{bad} must be rejected");
    }
}

#[test]
fn the_keys_of_each_health_kind_are_exactly_the_documented_ones() {
    let keys = |k: EventKind| -> Vec<String> {
        let json = serde_json::to_value(admitted(k)).unwrap();
        let (_, body) = json["kind"].as_object().unwrap().iter().next().map(|(n, b)| (n.clone(), b.clone())).unwrap();
        let mut ks: Vec<String> = match body {
            Value::Object(o) => o.keys().cloned().collect(),
            other => vec![format!("<{other}>")],
        };
        ks.sort();
        ks
    };
    assert_eq!(
        keys(EventKind::HealthEvent { category: HealthCategory::BugCheck, event_id: 1, code: None }),
        ["category", "code", "event_id"]
    );
    assert_eq!(
        keys(EventKind::InventoryChange { item: InventoryItem::OsBuild, previous: None, current: None }),
        ["current", "item", "previous"]
    );
    assert_eq!(keys(EventKind::PowerStatus { ac: None, charge_percent: None }), ["ac", "charge_percent"]);
    assert_eq!(
        keys(EventKind::HealthSample(full_sample())),
        [
            "cpu_freq_mhz",
            "cpu_load_pct",
            "cpu_perf_pct",
            "disk_busy_pct",
            "disk_latency_us",
            "gpu_pct",
            "mem_available_mb",
            "mem_commit_pct",
            "net_errors",
            "page_faults_per_sec",
            "passive_limit_pct",
            "thermal_kelvin"
        ]
    );
}

/// Todos os textos de um JSON: chaves e valores de texto.
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Object(o) => {
            for (k, x) in o {
                out.push(k.clone());
                strings(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        _ => {}
    }
}

#[test]
fn no_stored_health_text_looks_like_a_path_a_name_an_address_or_a_guid() {
    let mut seen = 0;
    for k in health_kinds() {
        let mut texts = Vec::new();
        strings(&serde_json::to_value(admitted(k)).unwrap(), &mut texts);
        for t in texts {
            seen += 1;
            for bad in ['\\', '/', '@', ':', ' ', '.', '{', '}', '-'] {
                assert!(!t.contains(bad), "{t:?} contains {bad:?}");
            }
            assert!(!t.contains("0000"), "{t:?}");
        }
    }
    assert!(seen > 100, "positive control: many texts were checked ({seen})");
}

#[test]
fn the_json_numbers_stay_inside_what_the_interface_can_read_without_loss() {
    // a interface (JavaScript) perde precisão acima de 2^53: nada de saúde passa disso
    let limit = (1u64 << 53) as f64;
    for k in health_kinds() {
        let json = serde_json::to_value(admitted(k)).unwrap();
        fn walk(v: &Value, limit: f64) {
            match v {
                Value::Number(n) => assert!(n.as_f64().unwrap().abs() < limit),
                Value::Object(o) => o.values().for_each(|x| walk(x, limit)),
                _ => {}
            }
        }
        walk(&json, limit);
    }
}
