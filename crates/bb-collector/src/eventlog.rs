//! Falhas e travamentos de aplicativos vindos do Windows Event Log (log "Application").
//!
//! Só dois campos saem daqui: o nome do executável e, nas falhas, o código de exceção
//! numérico. O texto da mensagem, caminhos de módulos e demais campos do XML são descartados
//! na hora. Este módulo é puro (sem Windows) para poder ser testado com XML sintético.

use bb_core::ExeName;

use crate::sample::CollectError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashKind {
    /// Evento 1000, provedor "Application Error".
    Crash,
    /// Evento 1002, provedor "Application Hang".
    Hang,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashRecord {
    /// Quando o Windows registrou o evento (UTC ms). Este é o instante usado no filtro de gravação.
    pub ts_utc_ms: i64,
    pub kind: CrashKind,
    pub exe_name: ExeName,
    pub exception_code: Option<u32>,
}

/// Fonte de falhas. Só é consultada enquanto o estado é `Recording`, e só interessa o que foi
/// registrado a partir de `since_utc_ms`.
pub trait CrashSource {
    fn poll(&mut self, since_utc_ms: i64) -> Result<Vec<CrashRecord>, CollectError>;
}

// ---------- datas ----------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "2026-09-30T13:05:22.1234567Z" -> milissegundos desde a época Unix.
pub fn ms_from_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, sec) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let frac_ms = if b.get(19) == Some(&b'.') {
        let digits: String = s[20..].chars().take_while(char::is_ascii_digit).take(3).collect();
        format!("{digits:0<3}").parse::<i64>().ok()?
    } else {
        0
    };
    Some(((days_from_civil(y, mo, d) * 24 + h) * 3600 + mi * 60 + sec) * 1000 + frac_ms)
}

/// Milissegundos desde a época Unix -> "2026-09-30T13:05:22.123Z".
pub fn iso_from_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms.rem_euclid(1000))
}

// ---------- XML ----------

pub(crate) fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

/// Valor de um atributo XML, com aspas simples ou duplas (o Windows usa aspas simples).
pub(crate) fn attr<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    for q in ['\'', '"'] {
        if let Some(v) = between(s, &format!("{name}={q}"), &q.to_string()) {
            return Some(v);
        }
    }
    None
}

/// Elementos `<Data>` do evento na ordem: (valor do atributo `Name` se houver, texto).
pub(crate) fn data_fields(xml: &str) -> Vec<(Option<&str>, &str)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<Data") {
        rest = &rest[i + 5..];
        // ignora `<DataFoo`
        if !matches!(rest.as_bytes().first(), Some(b'>') | Some(b' ') | Some(b'/')) {
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        let name = attr(tag, "Name");
        if tag.ends_with('/') {
            out.push((name, ""));
            continue;
        }
        let body = &rest[gt + 1..];
        let Some(end) = body.find("</Data>") else { break };
        out.push((name, &body[..end]));
        rest = &body[end..];
    }
    out
}

/// Campo por nome (`AppName`) e, se o evento não trouxer nomes, pela posição documentada.
fn field<'a>(data: &[(Option<&'a str>, &'a str)], name: &str, index: usize) -> Option<&'a str> {
    data.iter().find(|(n, _)| *n == Some(name)).map(|(_, v)| *v).or_else(|| data.get(index).map(|(_, v)| *v))
}

/// Interpreta o XML de um evento do log Application. Devolve `None` para qualquer evento que
/// não seja uma falha (1000) ou travamento (1002) dos provedores esperados, ou sem nome válido.
pub fn parse_event_xml(xml: &str) -> Option<CrashRecord> {
    let sys_end = xml.find("</System>").unwrap_or(xml.len());
    let system = &xml[..sys_end];
    let event_id: u32 = {
        let i = system.find("<EventID")?;
        let after = &system[i..];
        between(after, ">", "</EventID>")?.trim().parse().ok()?
    };
    let provider = attr(system, "Name").unwrap_or("");
    let kind = match (event_id, provider) {
        (1000, "Application Error") => CrashKind::Crash,
        (1002, "Application Hang") => CrashKind::Hang,
        _ => return None,
    };
    let ts_utc_ms = ms_from_iso(attr(system, "SystemTime")?)?;
    let data = data_fields(xml);
    let exe_name = ExeName::new(field(&data, "AppName", 0)?.trim()).ok()?;
    let exception_code = match kind {
        CrashKind::Crash => field(&data, "ExceptionCode", 6).and_then(|c| u32::from_str_radix(c.trim().trim_start_matches("0x"), 16).ok()),
        CrashKind::Hang => None,
    };
    Some(CrashRecord { ts_utc_ms, kind, exe_name, exception_code })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crash_xml(app: &str, code: &str, time: &str) -> String {
        format!(
            "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System>\
             <Provider Name='Application Error'/><EventID Qualifiers='0'>1000</EventID><Level>2</Level>\
             <TimeCreated SystemTime='{time}'/></System><EventData>\
             <Data>{app}</Data><Data>1.2.3.4</Data><Data>5f3a1b2c</Data><Data>ntdll.dll</Data>\
             <Data>10.0.26100.1</Data><Data>abcd1234</Data><Data>{code}</Data><Data>0000000000012345</Data>\
             <Data>1a2c</Data></EventData></Event>"
        )
        .replace('\'', "\"")
    }

    /// Formato REAL do Windows (conferido com `wevtutil`): aspas simples e `<Data Name='...'>`.
    /// Valores sintéticos; a estrutura é a de um evento 1000 verdadeiro.
    fn real_shape_crash() -> String {
        "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System>\
         <Provider Name='Application Error' Guid='{a0e9b465-b939-57d7-b27d-95d8e925ff57}'/>\
         <EventID>1000</EventID><Version>0</Version><Level>2</Level>\
         <TimeCreated SystemTime='2026-09-27T19:30:44.9640781Z'/><EventRecordID>1</EventRecordID>\
         <Channel>Application</Channel><Computer>SYNTH-PC</Computer>\
         <Security UserID='S-1-5-21-0-0-0-1000'/></System><EventData>\
         <Data Name='AppName'>Synth.Root.exe</Data><Data Name='AppVersion'>1.0.0.0</Data>\
         <Data Name='AppTimeStamp'>67d40000</Data><Data Name='ModuleName'>coreclr.dll</Data>\
         <Data Name='ModuleVersion'>8.0.1</Data><Data Name='ModuleTimeStamp'>67d4eaa9</Data>\
         <Data Name='ExceptionCode'>c0000005</Data><Data Name='FaultingOffset'>000000000018948f</Data>\
         <Data Name='ProcessId'>0x7fec</Data><Data Name='AppPath'>C:\\Synthetic\\Path\\Synth.Root.exe</Data>\
         <Data Name='PackageFullName'>Synth.Package_1.0</Data></EventData></Event>"
            .to_string()
    }

    #[test]
    fn parses_the_real_windows_xml_shape() {
        let r = parse_event_xml(&real_shape_crash()).expect("real-shaped event must parse");
        assert_eq!(r.kind, CrashKind::Crash);
        assert_eq!(r.exe_name.as_str(), "synth.root.exe");
        assert_eq!(r.exception_code, Some(0xc000_0005));
        assert_eq!(r.ts_utc_ms, ms_from_iso("2026-09-27T19:30:44.964Z").unwrap());
        // caminho, módulo, pacote e computador nunca chegam ao registro
        let dump = format!("{r:?}");
        for leaked in ["Synthetic", "coreclr", "Synth.Package", "SYNTH-PC", "S-1-5-21"] {
            assert!(!dump.contains(leaked), "{leaked} leaked");
        }
    }

    #[test]
    fn fields_are_found_by_name_even_if_the_order_differs() {
        let xml = real_shape_crash().replace(
            "<Data Name='AppName'>Synth.Root.exe</Data><Data Name='AppVersion'>1.0.0.0</Data>",
            "<Data Name='AppVersion'>1.0.0.0</Data><Data Name='AppName'>Synth.Root.exe</Data>",
        );
        assert_eq!(parse_event_xml(&xml).unwrap().exe_name.as_str(), "synth.root.exe");
    }

    #[test]
    fn epoch_conversions_match_known_values() {
        assert_eq!(ms_from_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(ms_from_iso("2000-01-01T00:00:00.000Z"), Some(946_684_800_000));
        assert_eq!(ms_from_iso("2024-02-29T12:30:45.5Z"), Some(1_709_209_845_500));
        assert_eq!(iso_from_ms(946_684_800_000), "2000-01-01T00:00:00.000Z");
        assert_eq!(iso_from_ms(1_709_209_845_500), "2024-02-29T12:30:45.500Z");
    }

    #[test]
    fn iso_round_trips_across_many_dates() {
        for ms in [0i64, 951_782_400_123, 1_600_000_000_999, 1_790_000_000_000, 4_102_444_799_000] {
            assert_eq!(ms_from_iso(&iso_from_ms(ms)), Some(ms), "{ms}");
        }
    }

    #[test]
    fn rejects_malformed_dates() {
        for bad in ["", "2026-13-01T00:00:00Z", "2026-09-30 13:05:22Z", "abc", "2026-09-30T25:00:00Z"] {
            assert_eq!(ms_from_iso(bad), None, "{bad}");
        }
    }

    #[test]
    fn parses_an_application_error_event() {
        let r = parse_event_xml(&crash_xml("Synth-App.EXE", "c0000005", "2000-01-01T00:00:01.5000000Z")).unwrap();
        assert_eq!(r.kind, CrashKind::Crash);
        assert_eq!(r.exe_name.as_str(), "synth-app.exe");
        assert_eq!(r.exception_code, Some(0xc000_0005));
        assert_eq!(r.ts_utc_ms, 946_684_801_500);
    }

    #[test]
    fn parses_an_application_hang_event() {
        let xml = "<Event><System><Provider Name=\"Application Hang\"/><EventID Qualifiers=\"0\">1002</EventID>\
                   <TimeCreated SystemTime=\"2000-01-01T00:00:00Z\"/></System><EventData>\
                   <Data>synth-app.exe</Data><Data>1.0</Data><Data>1a</Data></EventData></Event>";
        let r = parse_event_xml(xml).unwrap();
        assert_eq!((r.kind, r.exception_code), (CrashKind::Hang, None));
    }

    #[test]
    fn ignores_other_events_and_providers() {
        let other_id = crash_xml("a.exe", "c0000005", "2000-01-01T00:00:00Z").replace(">1000<", ">1001<");
        assert!(parse_event_xml(&other_id).is_none());
        let other_provider = crash_xml("a.exe", "c0000005", "2000-01-01T00:00:00Z").replace("Application Error", "Something Else");
        assert!(parse_event_xml(&other_provider).is_none());
    }

    #[test]
    fn rejects_names_that_look_like_paths_or_are_empty() {
        assert!(parse_event_xml(&crash_xml("C:\\Users\\x\\app.exe", "c0000005", "2000-01-01T00:00:00Z")).is_none());
        assert!(parse_event_xml(&crash_xml("", "c0000005", "2000-01-01T00:00:00Z")).is_none());
    }

    #[test]
    fn keeps_only_the_name_and_code_never_other_fields() {
        // O registro não tem onde guardar módulos, versões nem mensagem.
        let r = parse_event_xml(&crash_xml("a.exe", "c0000409", "2000-01-01T00:00:00Z")).unwrap();
        let dump = format!("{r:?}");
        assert!(!dump.contains("ntdll") && !dump.contains("1.2.3.4") && !dump.contains("5f3a1b2c"));
    }
}
