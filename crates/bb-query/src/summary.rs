//! Resumo em texto de um incidente, para colar num relato de bug.
//!
//! Sai SEMPRE de uma `ExportDoc` (a exportação já refiltrada pelas regras de agora), nunca direto do banco ou do journal:
//! o que a exportação não inclui, o resumo não inclui. Só entram enumerações fechadas, códigos e números; nenhum campo
//! de texto livre do documento é copiado (nem o resumo interno do incidente, nem anotações).

use crate::{ActivityRow, Detail, ExportDoc, TimelineRow};

/// Idioma do texto do resumo (o da interface).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryLang {
    En,
    PtBr,
}

/// Quantas linhas de eventos de saúde entram (as mais próximas do incidente).
const MAX_LISTED: usize = 8;

fn pick(lang: SummaryLang, en: &'static str, pt: &'static str) -> &'static str {
    match lang {
        SummaryLang::En => en,
        SummaryLang::PtBr => pt,
    }
}

/// Texto do tipo de incidente (`IncidentKind::as_str()`); `None` = tipo desconhecido.
pub fn kind_label(lang: SummaryLang, code: &str) -> Option<&'static str> {
    Some(match code {
        "manual" => pick(lang, "Manual capture", "Captura manual"),
        "cpu_sustained" => pick(lang, "Sustained high CPU", "CPU alta sustentada"),
        "memory_high" => pick(lang, "High memory use", "Uso alto de memória"),
        "unexpected_exit" => pick(lang, "Program crash", "Falha de programa"),
        "app_hang" => pick(lang, "Program not responding", "Programa sem resposta"),
        "unexpected_shutdown" => pick(lang, "Unexpected shutdown", "Desligamento inesperado"),
        "blue_screen" => pick(lang, "Blue screen", "Tela azul"),
        "hardware_error" => pick(lang, "Hardware error", "Erro de hardware"),
        "throttling" => pick(lang, "Thermal throttling", "Redução de desempenho por calor"),
        _ => return None,
    })
}

fn severity_label(lang: SummaryLang, code: &str) -> &'static str {
    match code {
        "info" => pick(lang, "info", "informação"),
        "warning" => pick(lang, "warning", "aviso"),
        "critical" => pick(lang, "critical", "crítico"),
        _ => pick(lang, "unknown", "desconhecida"),
    }
}

/// Texto da categoria de evento de saúde (enumeração fechada); `None` = categoria desconhecida.
pub fn category_label(lang: SummaryLang, code: &str) -> Option<&'static str> {
    Some(match code {
        "UnexpectedShutdown" => pick(lang, "Unexpected shutdown", "Desligamento inesperado"),
        "BugCheck" => pick(lang, "Blue screen (bug check)", "Tela azul (bug check)"),
        "HardwareError" => pick(lang, "Hardware error", "Erro de hardware"),
        "DisplayDriverReset" => pick(lang, "Display driver reset", "Reinício do driver de vídeo"),
        "DiskError" => pick(lang, "Disk error", "Erro de disco"),
        "FileSystemError" => pick(lang, "File system error", "Erro do sistema de arquivos"),
        "ServiceCrash" => pick(lang, "Service crash", "Falha de serviço"),
        "UpdateFailure" => pick(lang, "Update failure", "Falha de atualização"),
        "SleepEntered" => pick(lang, "Went to sleep", "Entrou em suspensão"),
        "Resumed" => pick(lang, "Resumed from sleep", "Voltou da suspensão"),
        _ => return None,
    })
}

fn item_label(lang: SummaryLang, code: &str) -> Option<&'static str> {
    Some(match code {
        "BiosVersion" => pick(lang, "BIOS version", "Versão da BIOS"),
        "BiosDate" => pick(lang, "BIOS date", "Data da BIOS"),
        "FirmwareType" => pick(lang, "Firmware type", "Tipo de firmware"),
        "SecureBoot" => pick(lang, "Secure Boot", "Secure Boot"),
        "OsBuild" => pick(lang, "Windows build", "Build do Windows"),
        "DeviceProblemCount" => pick(lang, "Devices with a driver problem", "Dispositivos com problema de driver"),
        "DeviceProblemCodes" => pick(lang, "Driver problem codes (mask)", "Códigos de problema de driver (máscara)"),
        _ => return None,
    })
}

/// "AAAA-MM-DD HH:MM UTC" de um instante em ms desde 1970 (calendário gregoriano proléptico).
pub fn utc_text(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Algoritmo de Howard Hinnant (civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

/// "-30 s", "+5 min": quando, em relação ao incidente.
fn offset_text(ms: i64) -> String {
    let sign = if ms < 0 { '-' } else { '+' };
    let abs = ms.unsigned_abs();
    if abs < 120_000 {
        format!("{sign}{} s", abs / 1000)
    } else if abs < 7_200_000 {
        format!("{sign}{} min", abs / 60_000)
    } else {
        format!("{sign}{} h", abs / 3_600_000)
    }
}

/// Valor de inventário em forma legível; o que não se sabe decodificar sai como o número.
fn inventory_value(item: &str, v: Option<u64>) -> String {
    let Some(v) = v else { return "?".into() };
    match item {
        "OsBuild" => format!("{}.{}", v >> 20, v & 0xF_FFFF),
        "BiosDate" => format!("{:04}-{:02}-{:02}", v / 10_000, v / 100 % 100, v % 100),
        _ => v.to_string(),
    }
}

fn find_sample(rows: &[TimelineRow]) -> Option<&TimelineRow> {
    rows.iter()
        .filter(|r| r.offset_ms <= 0 && matches!(r.row.detail, Detail::HealthSample { .. }))
        .max_by_key(|r| (r.offset_ms, r.row.seq))
}

fn sample_text(lang: SummaryLang, d: &Detail) -> Option<String> {
    let Detail::HealthSample {
        thermal_kelvin,
        passive_limit_pct,
        cpu_load_pct,
        cpu_perf_pct,
        cpu_freq_mhz,
        mem_commit_pct,
        mem_available_mb,
        page_faults_per_sec,
        disk_latency_us,
        disk_busy_pct,
        net_errors,
        gpu_pct,
    } = d
    else {
        return None;
    };
    let mut parts: Vec<String> = Vec::new();
    let mut add = |label: &str, v: &Option<u64>, unit: &str| {
        if let Some(v) = v {
            parts.push(format!("{label} {v}{unit}"));
        }
    };
    add(pick(lang, "CPU load", "carga da CPU"), cpu_load_pct, "%");
    add(pick(lang, "CPU performance", "desempenho da CPU"), cpu_perf_pct, "%");
    add(pick(lang, "CPU frequency", "frequência da CPU"), cpu_freq_mhz, " MHz");
    add(pick(lang, "thermal limit", "limite térmico"), passive_limit_pct, "%");
    add(pick(lang, "memory commit", "memória comprometida"), mem_commit_pct, "%");
    add(pick(lang, "memory available", "memória disponível"), mem_available_mb, " MB");
    add(pick(lang, "page faults", "falhas de página"), page_faults_per_sec, "/s");
    add(pick(lang, "disk latency", "latência do disco"), disk_latency_us, " µs");
    add(pick(lang, "disk busy", "disco ocupado"), disk_busy_pct, "%");
    add(pick(lang, "network errors", "erros de rede"), net_errors, "");
    add(pick(lang, "GPU", "GPU"), gpu_pct, "%");
    if let Some(k) = thermal_kelvin {
        parts.insert(0, format!("{} {} °C", pick(lang, "temperature", "temperatura"), (*k as i64) - 273));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn listed_line(lang: SummaryLang, r: &ActivityRow, offset_ms: i64) -> Option<String> {
    let when = offset_text(offset_ms);
    match &r.detail {
        Detail::HealthEvent { category, event_id, value } => {
            let label = category_label(lang, category)?;
            let code = match (category.as_str(), value) {
                ("BugCheck", Some(v)) => format!(", {} 0x{v:x}", pick(lang, "code", "código")),
                (_, Some(v)) => format!(", {} {v}", pick(lang, "value", "valor")),
                _ => String::new(),
            };
            Some(format!("- {when}: {label} ({} {event_id}{code})", pick(lang, "event", "evento")))
        }
        Detail::InventoryChange { item, previous, current } => {
            let label = item_label(lang, item)?;
            Some(format!("- {when}: {label}: {} -> {}", inventory_value(item, *previous), inventory_value(item, *current)))
        }
        _ => None,
    }
}

/// O resumo, em texto simples e pronto para colar. `os_build` é o build do Windows já lido pelo inventário (se o
/// inventário estiver ligado e tiver lido). Tudo vem de `doc`, que já passou pelas regras de privacidade de agora.
pub fn bug_report_summary(doc: &ExportDoc, app_version: &str, os_build: Option<u64>, lang: SummaryLang) -> String {
    let inc = &doc.incident;
    let mut out = Vec::new();
    out.push(pick(lang, "Developer Black Box - incident summary", "Developer Black Box - resumo do incidente").to_owned());
    // Só versões com a forma de um número de versão: nada que a pessoa ou um programa possa ter escrito.
    let version_ok = !app_version.is_empty() && app_version.len() <= 32 && app_version.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'+');
    if version_ok {
        out.push(format!("{}: {app_version}", pick(lang, "App version", "Versão do app")));
    }
    if let Some(b) = os_build {
        out.push(format!("{}: {}", pick(lang, "Windows build", "Build do Windows"), inventory_value("OsBuild", Some(b))));
    }
    let kind = kind_label(lang, &inc.kind).unwrap_or(pick(lang, "Incident", "Incidente"));
    out.push(format!("{}: {kind} ({})", pick(lang, "Incident", "Incidente"), severity_label(lang, &inc.severity)));
    out.push(format!("{}: {}", pick(lang, "When", "Quando"), utc_text(inc.created_utc_ms)));
    if let Some(exe) = inc.exe_name.as_deref().filter(|n| is_plain_exe_name(n)) {
        out.push(format!("{}: {exe}", pick(lang, "Program", "Programa")));
    }

    let mut near: Vec<(&TimelineRow, String)> = doc
        .health
        .events
        .iter()
        .filter_map(|r| listed_line(lang, &r.row, r.offset_ms).map(|l| (r, l)))
        .collect();
    near.sort_by_key(|(r, _)| (r.offset_ms.unsigned_abs(), r.row.seq));
    near.truncate(MAX_LISTED);
    near.sort_by_key(|(r, _)| (r.offset_ms, r.row.seq));
    if !near.is_empty() {
        out.push(String::new());
        out.push(pick(lang, "Machine health events around it:", "Eventos de saúde da máquina em volta:").to_owned());
        out.extend(near.into_iter().map(|(_, l)| l));
    }
    if let Some(sample) = find_sample(&doc.health.events).and_then(|r| sample_text(lang, &r.row.detail).map(|t| (r.offset_ms, t))) {
        out.push(String::new());
        out.push(format!("{} ({}): {}", pick(lang, "Last performance sample before it", "Última amostra de desempenho antes dele"), offset_text(sample.0), sample.1));
    }
    out.push(String::new());
    out.push(
        pick(
            lang,
            "Made from the export rules in effect when this was generated. It has no notes, user or computer names, paths or message texts.",
            "Gerado com as regras de exportação em vigor neste momento. Não tem anotações, nomes de usuário ou de computador, caminhos nem textos de mensagens.",
        )
        .to_owned(),
    );
    out.join("\n")
}

/// Nome de executável simples: letras, números e `. _ - +` e espaço, sem separador de caminho.
fn is_plain_exe_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+' | b' '))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HealthExport, IncidentDto};

    fn row(seq: u64, offset_ms: i64, detail: Detail) -> TimelineRow {
        TimelineRow { offset_ms, row: ActivityRow { seq, ts_utc_ms: 1_000_000_000_000 + offset_ms, kind: "x".into(), pid: None, exe_name: None, detail } }
    }

    fn doc(kind: &str, exe: Option<&str>, health: Vec<TimelineRow>) -> ExportDoc {
        ExportDoc {
            format: "developer-blackbox-export/2",
            exported_at_utc_ms: 1_000_000_100_000,
            incident: IncidentDto {
                id: 7,
                kind: kind.into(),
                severity: "critical".into(),
                created_utc_ms: 1_000_000_000_000,
                exe_name: exe.map(str::to_owned),
                summary: "blue_screen|209|1000000000000".into(),
                state: "new".into(),
                capture: "preserved".into(),
                post_until_utc_ms: 1_000_000_300_000,
                segments: vec![1, 2],
            },
            events: Vec::new(),
            dropped_events: 0,
            notes_included: false,
            health: HealthExport { from_utc_ms: 0, to_utc_ms: 0, events: health, dropped: 0, truncated: false },
        }
    }

    fn bugcheck(seq: u64, offset: i64) -> TimelineRow {
        row(seq, offset, Detail::HealthEvent { category: "BugCheck".into(), event_id: 1001, value: Some(0xd1) })
    }

    fn sample(seq: u64, offset: i64, cpu: u64) -> TimelineRow {
        row(
            seq,
            offset,
            Detail::HealthSample {
                thermal_kelvin: Some(318),
                passive_limit_pct: Some(100),
                cpu_load_pct: Some(cpu),
                cpu_perf_pct: None,
                cpu_freq_mhz: None,
                mem_commit_pct: Some(54),
                mem_available_mb: None,
                page_faults_per_sec: None,
                disk_latency_us: None,
                disk_busy_pct: None,
                net_errors: None,
                gpu_pct: None,
            },
        )
    }

    #[test]
    fn utc_text_is_the_calendar_date_of_the_instant() {
        assert_eq!(utc_text(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_text(1_000_000_000_000), "2001-09-09 01:46 UTC");
        assert_eq!(utc_text(951_782_400_000), "2000-02-29 00:00 UTC", "leap day");
        assert_eq!(utc_text(-1), "1969-12-31 23:59 UTC");
    }

    #[test]
    fn offsets_read_naturally() {
        assert_eq!(offset_text(-30_000), "-30 s");
        assert_eq!(offset_text(0), "+0 s");
        assert_eq!(offset_text(5 * 60_000), "+5 min");
        assert_eq!(offset_text(-3 * 3_600_000), "-3 h");
    }

    #[test]
    fn the_summary_has_the_facts_and_nothing_else() {
        let d = doc("blue_screen", None, vec![sample(1, -35_000, 23), bugcheck(2, 0)]);
        let s = bug_report_summary(&d, "0.5.0", Some((26100u64 << 20) | 1742), SummaryLang::En);
        assert!(s.contains("App version: 0.5.0"), "{s}");
        assert!(s.contains("Windows build: 26100.1742"), "{s}");
        assert!(s.contains("Incident: Blue screen (critical)"), "{s}");
        assert!(s.contains("When: 2001-09-09 01:46 UTC"), "{s}");
        assert!(s.contains("- +0 s: Blue screen (bug check) (event 1001, code 0xd1)"), "{s}");
        assert!(s.contains("temperature 45 °C") && s.contains("CPU load 23%") && s.contains("memory commit 54%"), "{s}");
        assert!(!s.contains("Program:"), "no program when the incident has none");
        assert!(!s.contains("blue_screen|"), "the internal summary is never copied");
    }

    #[test]
    fn it_is_available_in_both_languages_with_no_untranslated_fixed_text() {
        let d = doc("unexpected_shutdown", None, vec![row(1, -10_000, Detail::HealthEvent { category: "UnexpectedShutdown".into(), event_id: 6008, value: None })]);
        let en = bug_report_summary(&d, "0.5.0", None, SummaryLang::En);
        let pt = bug_report_summary(&d, "0.5.0", None, SummaryLang::PtBr);
        assert!(en.contains("incident summary") && pt.contains("resumo do incidente"));
        assert!(pt.contains("Desligamento inesperado") && pt.contains("Quando:"));
        assert_ne!(en, pt);
    }

    #[test]
    fn every_kind_and_category_has_its_own_text_in_both_languages() {
        let kinds = ["manual", "cpu_sustained", "memory_high", "unexpected_exit", "app_hang", "unexpected_shutdown", "blue_screen", "hardware_error", "throttling"];
        let cats = [
            "UnexpectedShutdown", "BugCheck", "HardwareError", "DisplayDriverReset", "DiskError", "FileSystemError", "ServiceCrash",
            "UpdateFailure", "SleepEntered", "Resumed",
        ];
        for lang in [SummaryLang::En, SummaryLang::PtBr] {
            let mut seen = std::collections::HashSet::new();
            for k in kinds {
                assert!(seen.insert(kind_label(lang, k).unwrap_or_else(|| panic!("{k}"))), "{k} repeats a text");
            }
            for c in cats {
                category_label(lang, c).unwrap_or_else(|| panic!("{c}"));
            }
        }
        for k in kinds {
            assert_ne!(kind_label(SummaryLang::En, k), kind_label(SummaryLang::PtBr, k), "{k} is translated");
        }
        for c in cats {
            assert_ne!(category_label(SummaryLang::En, c), category_label(SummaryLang::PtBr, c), "{c} is translated");
        }
        assert!(kind_label(SummaryLang::En, "other").is_none() && category_label(SummaryLang::En, "other").is_none());
    }

    #[test]
    fn the_program_name_comes_only_from_the_refiltered_export_and_only_if_plain() {
        let named = doc("cpu_sustained", Some("synth-app.exe"), vec![]);
        assert!(bug_report_summary(&named, "0.5.0", None, SummaryLang::En).contains("Program: synth-app.exe"));
        // a exportação já devolve None para um programa excluído hoje: o resumo não cita
        let excluded = doc("cpu_sustained", None, vec![]);
        assert!(!bug_report_summary(&excluded, "0.5.0", None, SummaryLang::En).contains("Program:"));
        // e um valor que não é um nome simples (caminho, quebra de linha) nunca é copiado
        for odd in ["C:\\Users\\someone\\app.exe", "app.exe\nsecret", "a/b.exe", ""] {
            let d = doc("cpu_sustained", Some(odd), vec![]);
            assert!(!bug_report_summary(&d, "0.5.0", None, SummaryLang::En).contains("Program:"), "{odd:?}");
        }
    }

    #[test]
    fn a_version_that_is_not_a_version_is_left_out() {
        let d = doc("blue_screen", None, vec![]);
        for odd in ["", "0.5.0\nWindows build: 1", "C:\\x", &"1".repeat(40)] {
            assert!(!bug_report_summary(&d, odd, None, SummaryLang::En).contains("App version"), "{odd:?}");
        }
        assert!(bug_report_summary(&d, "0.6.0-rc.1", None, SummaryLang::En).contains("App version: 0.6.0-rc.1"));
    }

    #[test]
    fn it_lists_only_the_nearest_health_events_and_skips_unknown_ones() {
        let mut rows: Vec<TimelineRow> = (0..20).map(|i| bugcheck(i, -(i as i64) * 1000)).collect();
        rows.push(row(99, -500, Detail::HealthEvent { category: "SomethingNew".into(), event_id: 1, value: None }));
        rows.push(row(100, -400, Detail::InventoryChange { item: "OsBuild".into(), previous: Some((26100 << 20) | 1), current: Some((26100 << 20) | 2) }));
        let s = bug_report_summary(&doc("blue_screen", None, rows), "0.5.0", None, SummaryLang::En);
        let listed = s.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(listed, MAX_LISTED, "{s}");
        assert!(!s.contains("SomethingNew"), "an unknown category is not copied");
        assert!(s.contains("Windows build: 26100.1 -> 26100.2"), "the nearest inventory change is listed and decoded: {s}");
    }

    #[test]
    fn only_a_sample_from_before_the_incident_is_shown_and_the_latest_one() {
        let rows = vec![sample(1, -90_000, 10), sample(2, -30_000, 77), sample(3, 20_000, 99)];
        let s = bug_report_summary(&doc("blue_screen", None, rows), "0.5.0", None, SummaryLang::En);
        assert!(s.contains("CPU load 77%") && !s.contains("CPU load 10%") && !s.contains("CPU load 99%"), "{s}");
        assert!(s.contains("(-30 s)"), "{s}");
        let none = bug_report_summary(&doc("blue_screen", None, vec![sample(3, 20_000, 99)]), "0.5.0", None, SummaryLang::En);
        assert!(!none.contains("performance sample"), "no sample before the incident means no line");
    }

    #[test]
    fn nothing_from_the_incident_text_or_notes_is_copied() {
        let mut d = doc("blue_screen", None, vec![]);
        d.incident.summary = "SECRET-INTERNAL-SUMMARY".into();
        d.incident.state = "SECRET-STATE".into();
        let s = bug_report_summary(&d, "0.5.0", None, SummaryLang::En);
        assert!(!s.contains("SECRET"), "{s}");
        // tipo e gravidade desconhecidos viram texto genérico, nunca o valor cru
        d.incident.kind = "SECRET-KIND".into();
        d.incident.severity = "SECRET-SEVERITY".into();
        let s = bug_report_summary(&d, "0.5.0", None, SummaryLang::En);
        assert!(!s.contains("SECRET"), "{s}");
    }
}
