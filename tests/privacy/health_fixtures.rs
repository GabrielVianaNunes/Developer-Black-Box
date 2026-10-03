//! Fixtures SINTÉTICAS da saúde da máquina (tests/fixtures/health): XML de eventos do Windows, valores de contadores e
//! leituras de inventário. Cada regra da lista fixa do Event Log precisa de uma fixture (e vice-versa), então remover ou
//! acrescentar uma regra sem atualizar as fixtures falha aqui. Nada nelas vem de uma máquina real.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use bb_collector::healthlog::{parse_health_xml, RULES};
use bb_collector::inventory::{self, HASHED_FLAG};
use bb_collector::telemetry::{RawCounters, SampleBuilder};
use bb_core::{HealthSample, InventoryItem};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/health")
}

fn files(dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(root().join(dir))
        .unwrap_or_else(|e| panic!("fixture dir {dir}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == ext))
        .collect();
    v.sort();
    v
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'))
}

// ---------- Event Log ----------

fn rule_file(provider: &str, id: u16) -> String {
    format!("{}__{id}.xml", provider.replace(' ', "_"))
}

#[test]
fn every_rule_has_exactly_one_fixture_and_every_fixture_belongs_to_a_rule() {
    let from_rules: BTreeSet<String> = RULES.iter().map(|r| rule_file(r.provider, r.event_id)).collect();
    let from_disk: BTreeSet<String> = files("eventlog", "xml").iter().map(|p| name(p)).collect();
    assert_eq!(from_disk, from_rules, "a rule without a fixture (or a fixture without a rule) is a hole in the tests");
    assert_eq!(from_rules.len(), RULES.len(), "no two rules share a provider and ID");
}

#[test]
fn each_event_fixture_becomes_exactly_the_expected_record() {
    let manifest = fs::read_to_string(root().join("eventlog/expected.txt")).unwrap();
    let mut expected: BTreeMap<String, (String, u16, Option<u32>, i64)> = BTreeMap::new();
    for l in lines(&manifest) {
        let p: Vec<&str> = l.split('|').map(str::trim).collect();
        assert_eq!(p.len(), 5, "bad manifest line: {l}");
        let code = (p[3] != "-").then(|| p[3].parse().unwrap());
        expected.insert(p[0].to_owned(), (p[1].to_owned(), p[2].parse().unwrap(), code, p[4].parse().unwrap()));
    }
    for f in files("eventlog", "xml") {
        let want = expected.remove(&name(&f)).unwrap_or_else(|| panic!("{} is not in the manifest", name(&f)));
        let got = parse_health_xml(&fs::read_to_string(&f).unwrap()).unwrap_or_else(|| panic!("{} must parse", name(&f)));
        assert_eq!(format!("{:?}", got.category), want.0, "{}", name(&f));
        assert_eq!((got.event_id, got.code, got.ts_utc_ms), (want.1, want.2, want.3), "{}", name(&f));
        // o registro não tem onde guardar nada do texto sintético que o XML carrega
        let dump = format!("{got:?}").to_lowercase();
        for leaked in ["synth", "c:\\", "kb0000000", "{0000", "s-1-5"] {
            assert!(!dump.contains(leaked), "{}: {leaked} leaked into {dump}", name(&f));
        }
    }
    assert!(expected.is_empty(), "manifest lines without a file: {:?}", expected.keys().collect::<Vec<_>>());
}

#[test]
fn events_that_are_not_on_the_list_are_ignored() {
    let dir = root().join("eventlog/ignored");
    let all: Vec<PathBuf> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
    assert!(all.len() >= 5, "positive control: the ignored fixtures exist");
    for f in all {
        assert!(parse_health_xml(&fs::read_to_string(&f).unwrap()).is_none(), "{} must be ignored", name(&f));
    }
}

// ---------- contadores ----------

fn list(v: &str) -> Vec<f64> {
    v.split(',').filter(|s| !s.is_empty()).map(|s| s.trim().parse().unwrap()).collect()
}

fn raw_from(text: &str) -> (RawCounters, BTreeMap<String, String>) {
    let mut raw = RawCounters::default();
    let mut expect = BTreeMap::new();
    for l in lines(text) {
        let (k, v) = l.split_once('=').unwrap_or_else(|| panic!("bad line: {l}"));
        let one = || Some(v.trim().parse::<f64>().unwrap());
        match k {
            "temperatures_k" => raw.temperatures_k = list(v),
            "passive_limits_pct" => raw.passive_limits_pct = list(v),
            "cpu_load_pct" => raw.cpu_load_pct = one(),
            "cpu_perf_pct" => raw.cpu_perf_pct = one(),
            "cpu_freq_mhz" => raw.cpu_freq_mhz = one(),
            "mem_commit_pct" => raw.mem_commit_pct = one(),
            "mem_available_mb" => raw.mem_available_mb = one(),
            "page_faults_per_sec" => raw.page_faults_per_sec = one(),
            "disk_latency_sec" => raw.disk_latency_sec = one(),
            "disk_busy_pct" => raw.disk_busy_pct = one(),
            "net_errors_total" => raw.net_errors_total = one(),
            "gpu" => {
                raw.gpu_engines = v
                    .split(';')
                    .map(|e| {
                        let (n, x) = e.rsplit_once(':').unwrap();
                        (n.to_owned(), x.parse().unwrap())
                    })
                    .collect()
            }
            k if k.starts_with("expect.") => {
                expect.insert(k.trim_start_matches("expect.").to_owned(), v.trim().to_owned());
            }
            other => panic!("unknown fixture key {other}"),
        }
    }
    (raw, expect)
}

fn field(s: &HealthSample, key: &str) -> Option<u64> {
    match key {
        "thermal_kelvin" => s.thermal_kelvin.map(u64::from),
        "passive_limit_pct" => s.passive_limit_pct.map(u64::from),
        "cpu_load_pct" => s.cpu_load_pct.map(u64::from),
        "cpu_perf_pct" => s.cpu_perf_pct.map(u64::from),
        "cpu_freq_mhz" => s.cpu_freq_mhz.map(u64::from),
        "mem_commit_pct" => s.mem_commit_pct.map(u64::from),
        "mem_available_mb" => s.mem_available_mb.map(u64::from),
        "page_faults_per_sec" => s.page_faults_per_sec.map(u64::from),
        "disk_latency_us" => s.disk_latency_us.map(u64::from),
        "disk_busy_pct" => s.disk_busy_pct.map(u64::from),
        "net_errors" => s.net_errors.map(u64::from),
        "gpu_pct" => s.gpu_pct.map(u64::from),
        other => panic!("unknown sample field {other}"),
    }
}

#[test]
fn each_counter_fixture_becomes_the_expected_sample() {
    let fixtures = files("counters", "txt");
    assert!(fixtures.len() >= 5, "positive control: the counter fixtures exist");
    for f in fixtures {
        let (raw, expect) = raw_from(&fs::read_to_string(&f).unwrap());
        assert!(!expect.is_empty(), "{}: a fixture must state what it expects", name(&f));
        let sample = SampleBuilder::new().build(&raw);
        for (k, v) in &expect {
            if k == "empty" {
                assert_eq!(sample.is_empty(), v == "true", "{}: empty", name(&f));
            } else if v == "none" {
                assert_eq!(field(&sample, k), None, "{}: {k}", name(&f));
            } else {
                assert_eq!(field(&sample, k), Some(v.parse().unwrap()), "{}: {k}", name(&f));
            }
        }
    }
}

// ---------- inventário ----------

fn item(name: &str) -> InventoryItem {
    *inventory::ALL.iter().find(|i| format!("{i:?}") == name).unwrap_or_else(|| panic!("unknown item {name}"))
}

#[test]
fn each_inventory_fixture_becomes_the_expected_numbers() {
    let fixtures = files("inventory", "txt");
    assert!(fixtures.len() >= 4, "positive control: the inventory fixtures exist");
    for f in fixtures {
        let text = fs::read_to_string(&f).unwrap();
        let (mut bios_v, mut bios_d, mut fw, mut sb, mut os, mut codes) = (None, None, None, None, None, None);
        let mut expect = Vec::new();
        for l in lines(&text) {
            let (k, v) = l.split_once('=').unwrap();
            match k {
                "bios_version" => bios_v = Some(v.to_owned()),
                "bios_date" => bios_d = Some(v.to_owned()),
                "firmware" => fw = Some(v.parse::<u64>().unwrap()),
                "secure_boot" => sb = Some(v == "1"),
                "os" => {
                    let (b, r) = v.split_once('.').unwrap();
                    os = Some((b.parse::<u32>().unwrap(), r.parse::<u32>().unwrap()));
                }
                "problem_codes" => codes = Some(v.split(',').map(|c| c.parse::<u32>().unwrap()).collect::<Vec<_>>()),
                k if k.starts_with("expect.") => expect.push((k.trim_start_matches("expect.").to_owned(), v.to_owned())),
                other => panic!("unknown fixture key {other}"),
            }
        }
        let snap = inventory::snapshot(bios_v.as_deref(), bios_d.as_deref(), fw, sb, os, codes.as_deref());
        assert!(!expect.is_empty(), "{}: a fixture must state what it expects", name(&f));
        for (k, v) in expect {
            let got = snap.get(item(&k));
            match v.as_str() {
                "none" => assert_eq!(got, None, "{}: {k}", name(&f)),
                "hashed" => assert!(got.is_some_and(|g| g >= HASHED_FLAG), "{}: {k} must be a flagged hash, got {got:?}", name(&f)),
                n => assert_eq!(got, Some(n.parse().unwrap()), "{}: {k}", name(&f)),
            }
        }
    }
}

#[test]
fn the_fixtures_hold_only_synthetic_markers() {
    // cinto e suspensório do script de higiene (Node): nenhum nome de computador/usuário que não seja sintético
    for dir in ["eventlog", "eventlog/ignored", "counters", "inventory"] {
        for e in fs::read_dir(root().join(dir)).unwrap().filter_map(|e| e.ok()) {
            if !e.path().is_file() {
                continue;
            }
            let text = fs::read_to_string(e.path()).unwrap();
            for tag in ["Computer"] {
                for part in text.split(&format!("<{tag}>")).skip(1) {
                    let v = part.split('<').next().unwrap();
                    assert!(v.starts_with("SYNTH"), "{}: {tag} must be synthetic: {v}", name(&e.path()));
                }
            }
        }
    }
}
