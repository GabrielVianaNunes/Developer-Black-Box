//! Eventos de saúde da máquina vindos do Windows Event Log (log "System").
//!
//! Lista FIXA de provedor + ID. De cada evento só saem a categoria, o ID e, quando a regra manda, UM número
//! (ex. o código da tela azul). Mensagem, nomes de serviço, caminhos, nomes de usuário e de computador do XML são
//! descartados na hora: o registro não tem onde guardá-los. Este módulo é puro (sem Windows) para ser testado com
//! XML sintético.

use bb_core::HealthCategory;

use crate::eventlog::{attr, between, data_fields, ms_from_iso};
use crate::sample::CollectError;

/// De onde sai o número opcional do registro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeSource {
    /// O evento não carrega número que valha guardar.
    None,
    /// `<Data Name='...'>` em decimal.
    Decimal(&'static str),
    /// `<Data Name='...'>` em hexadecimal (com ou sem `0x`).
    Hex(&'static str),
    /// `<Data Name='...'>` começando por um hexadecimal `0x...` (o resto do texto é descartado).
    LeadingHex(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub provider: &'static str,
    pub event_id: u16,
    pub category: HealthCategory,
    pub code: CodeSource,
}

const fn rule(provider: &'static str, event_id: u16, category: HealthCategory, code: CodeSource) -> Rule {
    Rule { provider, event_id, category, code }
}

/// A lista fixa. Cada linha documentada em README/CHANGELOG; nada fora dela é lido.
/// ATENÇÃO: os IDs e nomes de campo abaixo seguem a documentação pública do Windows; só os de falhas
/// de aplicativo foram conferidos numa máquina real nas versões anteriores. Estes ainda não.
pub const RULES: &[Rule] = &[
    rule("Microsoft-Windows-Kernel-Power", 41, HealthCategory::UnexpectedShutdown, CodeSource::Decimal("BugcheckCode")),
    rule("EventLog", 6008, HealthCategory::UnexpectedShutdown, CodeSource::None),
    rule("Microsoft-Windows-WER-SystemErrorReporting", 1001, HealthCategory::BugCheck, CodeSource::LeadingHex("param1")),
    rule("Microsoft-Windows-WHEA-Logger", 1, HealthCategory::HardwareError, CodeSource::None),
    rule("Microsoft-Windows-WHEA-Logger", 17, HealthCategory::HardwareError, CodeSource::None),
    rule("Microsoft-Windows-WHEA-Logger", 18, HealthCategory::HardwareError, CodeSource::None),
    rule("Microsoft-Windows-WHEA-Logger", 19, HealthCategory::HardwareError, CodeSource::None),
    rule("Microsoft-Windows-WHEA-Logger", 20, HealthCategory::HardwareError, CodeSource::None),
    rule("Microsoft-Windows-WHEA-Logger", 47, HealthCategory::HardwareError, CodeSource::None),
    rule("Display", 4101, HealthCategory::DisplayDriverReset, CodeSource::None),
    rule("disk", 7, HealthCategory::DiskError, CodeSource::None),
    rule("disk", 11, HealthCategory::DiskError, CodeSource::None),
    rule("disk", 51, HealthCategory::DiskError, CodeSource::None),
    rule("disk", 153, HealthCategory::DiskError, CodeSource::None),
    rule("Ntfs", 55, HealthCategory::FileSystemError, CodeSource::None),
    rule("Ntfs", 98, HealthCategory::FileSystemError, CodeSource::None),
    rule("Ntfs", 140, HealthCategory::FileSystemError, CodeSource::None),
    // O número é a contagem de quedas do serviço (param2); o nome do serviço (param1) nunca é lido.
    rule("Service Control Manager", 7031, HealthCategory::ServiceCrash, CodeSource::Decimal("param2")),
    rule("Service Control Manager", 7034, HealthCategory::ServiceCrash, CodeSource::Decimal("param2")),
    rule("Microsoft-Windows-WindowsUpdateClient", 20, HealthCategory::UpdateFailure, CodeSource::Hex("errorCode")),
    // Suspensão e retomada. O número de 42 é o estado de destino (3 = suspensão, 4 = hibernação), segundo a documentação.
    rule("Microsoft-Windows-Kernel-Power", 42, HealthCategory::SleepEntered, CodeSource::Decimal("TargetState")),
    rule("Microsoft-Windows-Kernel-Power", 107, HealthCategory::Resumed, CodeSource::None),
];

/// Um evento de saúde já reduzido ao que pode ser guardado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthRecord {
    /// Quando o Windows registrou o evento (UTC ms).
    pub ts_utc_ms: i64,
    pub category: HealthCategory,
    pub event_id: u16,
    pub code: Option<u32>,
}

/// Fonte de eventos de saúde. Erro = fonte indisponível (o motor segue sem ela, sem alarme).
pub trait HealthSource {
    fn poll(&mut self, since_utc_ms: i64) -> Result<Vec<HealthRecord>, CollectError>;
}

/// Filtro XPath com os IDs da lista fixa (sem repetir), para o Windows já devolver só o que interessa.
pub fn id_filter() -> String {
    let mut ids: Vec<u16> = RULES.iter().map(|r| r.event_id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.iter().map(|i| format!("EventID={i}")).collect::<Vec<_>>().join(" or ")
}

fn parse_hex(s: &str) -> Option<u32> {
    let t = s.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if t.is_empty() || t.len() > 8 {
        return None;
    }
    u32::from_str_radix(t, 16).ok()
}

fn code_of(src: CodeSource, data: &[(Option<&str>, &str)]) -> Option<u32> {
    let value = |name: &str| data.iter().find(|(n, _)| *n == Some(name)).map(|(_, v)| *v);
    match src {
        CodeSource::None => None,
        CodeSource::Decimal(name) => value(name)?.trim().parse::<u32>().ok(),
        CodeSource::Hex(name) => parse_hex(value(name)?),
        CodeSource::LeadingHex(name) => {
            let v = value(name)?.trim();
            // só o primeiro "token"; o restante (parâmetros entre parênteses etc.) é descartado
            let first = v.split_whitespace().next()?;
            if !first.to_ascii_lowercase().starts_with("0x") {
                return None;
            }
            parse_hex(first)
        }
    }
}

/// Interpreta o XML de um evento do log System. Devolve `None` para qualquer evento fora da lista fixa
/// (provedor e ID precisam bater juntos) ou com data inválida.
pub fn parse_health_xml(xml: &str) -> Option<HealthRecord> {
    let sys_end = xml.find("</System>").unwrap_or(xml.len());
    let system = &xml[..sys_end];
    let event_id: u16 = {
        let i = system.find("<EventID")?;
        between(&system[i..], ">", "</EventID>")?.trim().parse().ok()?
    };
    let provider = attr(system, "Name")?;
    let rule = RULES.iter().find(|r| r.event_id == event_id && r.provider == provider)?;
    let ts_utc_ms = ms_from_iso(attr(system, "SystemTime")?)?;
    let code = code_of(rule.code, &data_fields(xml));
    Some(HealthRecord { ts_utc_ms, category: rule.category, event_id, code })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XML no formato real (aspas simples, `<Data Name=...>`), com valores 100% sintéticos e com "sujeira" de propósito
    /// (usuário, computador, caminho, nome de serviço) para provar que nada disso sobrevive.
    fn xml(provider: &str, id: u16, data: &str) -> String {
        format!(
            "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System>\
             <Provider Name='{provider}' Guid='{{00000000-0000-0000-0000-000000000000}}'/>\
             <EventID Qualifiers='0'>{id}</EventID><Level>2</Level>\
             <TimeCreated SystemTime='2026-09-27T19:30:44.9640781Z'/><Channel>System</Channel>\
             <Computer>SYNTH-PC</Computer><Security UserID='S-1-5-21-0-0-0-1000'/></System>\
             <EventData>{data}</EventData><RenderingInfo><Message>C:\\Users\\synth-user\\secret.txt falhou</Message></RenderingInfo></Event>"
        )
    }

    const TS: i64 = 1_790_537_444_964;

    fn rec(provider: &str, id: u16, data: &str) -> Option<HealthRecord> {
        parse_health_xml(&xml(provider, id, data))
    }

    #[test]
    fn kernel_power_41_keeps_the_bugcheck_code() {
        let r = rec("Microsoft-Windows-Kernel-Power", 41, "<Data Name='BugcheckCode'>209</Data><Data Name='PowerButtonTimestamp'>0</Data>").unwrap();
        assert_eq!(r, HealthRecord { ts_utc_ms: TS, category: HealthCategory::UnexpectedShutdown, event_id: 41, code: Some(209) });
    }

    #[test]
    fn kernel_power_41_without_a_code_still_records_the_shutdown() {
        let r = rec("Microsoft-Windows-Kernel-Power", 41, "<Data Name='PowerButtonTimestamp'>0</Data>").unwrap();
        assert_eq!((r.category, r.code), (HealthCategory::UnexpectedShutdown, None));
    }

    #[test]
    fn eventlog_6008_has_no_code_and_drops_the_text() {
        let r = rec("EventLog", 6008, "<Data>10:15:00</Data><Data>27/09/2026</Data>").unwrap();
        assert_eq!((r.category, r.event_id, r.code), (HealthCategory::UnexpectedShutdown, 6008, None));
    }

    #[test]
    fn bugcheck_1001_keeps_only_the_leading_hex_code() {
        let data = "<Data Name='param1'>0x000000d1 (0xfffff80000000000, 0x2, 0x0, 0xfffff80000000001)</Data>\
                    <Data Name='param2'>C:\\Windows\\MEMORY.DMP</Data><Data Name='param3'>00000000-0000-0000-0000-000000000000</Data>";
        let r = rec("Microsoft-Windows-WER-SystemErrorReporting", 1001, data).unwrap();
        assert_eq!((r.category, r.code), (HealthCategory::BugCheck, Some(0xd1)));
    }

    #[test]
    fn bugcheck_1001_with_a_non_numeric_param_keeps_no_code() {
        let r = rec("Microsoft-Windows-WER-SystemErrorReporting", 1001, "<Data Name='param1'>some synth text</Data>").unwrap();
        assert_eq!(r.code, None);
    }

    #[test]
    fn every_whea_id_is_recognized() {
        for id in [1u16, 17, 18, 19, 20, 47] {
            let r = rec("Microsoft-Windows-WHEA-Logger", id, "<Data Name='ErrorSource'>2</Data>").unwrap_or_else(|| panic!("{id}"));
            assert_eq!((r.category, r.event_id, r.code), (HealthCategory::HardwareError, id, None));
        }
    }

    #[test]
    fn display_4101_is_a_driver_reset() {
        let r = rec("Display", 4101, "<Data>synthetic adapter name</Data>").unwrap();
        assert_eq!((r.category, r.code), (HealthCategory::DisplayDriverReset, None));
        assert!(!format!("{r:?}").contains("synthetic"));
    }

    #[test]
    fn disk_and_ntfs_ids_are_recognized() {
        for id in [7u16, 11, 51, 153] {
            assert_eq!(rec("disk", id, "<Data>\\Device\\Harddisk0\\DR0</Data>").unwrap().category, HealthCategory::DiskError, "{id}");
        }
        for id in [55u16, 98, 140] {
            assert_eq!(rec("Ntfs", id, "<Data>C:</Data>").unwrap().category, HealthCategory::FileSystemError, "{id}");
        }
    }

    #[test]
    fn service_crash_keeps_the_count_and_never_the_service_name() {
        let data = "<Data Name='param1'>SynthServiceNameUniqueXyz</Data><Data Name='param2'>3</Data><Data Name='param3'>%%7</Data>";
        for id in [7031u16, 7034] {
            let r = rec("Service Control Manager", id, data).unwrap();
            assert_eq!((r.category, r.code), (HealthCategory::ServiceCrash, Some(3)));
            assert!(!format!("{r:?}").contains("Synth"));
        }
    }

    #[test]
    fn update_failure_keeps_the_hex_error_code_only() {
        let data = "<Data Name='updateTitle'>Synthetic Update KB0000000</Data><Data Name='updateGuid'>00000000-0000-0000-0000-000000000000</Data><Data Name='errorCode'>0x80070005</Data>";
        let r = rec("Microsoft-Windows-WindowsUpdateClient", 20, data).unwrap();
        assert_eq!((r.category, r.code), (HealthCategory::UpdateFailure, Some(0x8007_0005)));
        assert!(!format!("{r:?}").contains("KB0000000"));
    }

    #[test]
    fn sleep_keeps_only_the_target_state_and_resume_keeps_nothing() {
        let r = rec("Microsoft-Windows-Kernel-Power", 42, "<Data Name='TargetState'>4</Data><Data Name='EffectiveState'>4</Data><Data Name='Reason'>0</Data>").unwrap();
        assert_eq!((r.category, r.event_id, r.code), (HealthCategory::SleepEntered, 42, Some(4)));
        let r = rec("Microsoft-Windows-Kernel-Power", 107, "<Data Name='TargetState'>3</Data><Data Name='ResumeTime'>x</Data>").unwrap();
        assert_eq!((r.category, r.event_id, r.code), (HealthCategory::Resumed, 107, None));
    }

    #[test]
    fn provider_and_id_must_match_together() {
        // ID da lista com provedor de fora, e provedor da lista com ID de fora: ignorados.
        assert!(rec("Some-Other-Provider", 41, "").is_none());
        assert!(rec("Microsoft-Windows-Kernel-Power", 43, "").is_none());
        assert!(rec("disk", 41, "").is_none());
    }

    #[test]
    fn every_rule_in_the_list_parses_and_nothing_else_does() {
        for r in RULES {
            let got = rec(r.provider, r.event_id, "").unwrap_or_else(|| panic!("{} {}", r.provider, r.event_id));
            assert_eq!((got.category, got.event_id), (r.category, r.event_id));
        }
        for id in [0u16, 1000, 1002, 4624, 6005, 6006, 6013, 7036, 9999] {
            assert!(rec("Microsoft-Windows-Kernel-Power", id, "").is_none(), "{id}");
        }
    }

    #[test]
    fn free_text_never_survives_in_the_record() {
        let data = "<Data Name='BugcheckCode'>1</Data><Data Name='Note'>C:\\Users\\synth-user\\a.txt</Data>";
        let r = rec("Microsoft-Windows-Kernel-Power", 41, data).unwrap();
        let dump = format!("{r:?}");
        for leaked in ["synth-user", "SYNTH-PC", "S-1-5-21", "secret", "a.txt", "falhou", "Guid"] {
            assert!(!dump.contains(leaked), "{leaked} leaked");
        }
    }

    #[test]
    fn malformed_input_is_ignored() {
        for bad in ["", "<Event/>", "<Event><System><EventID>41</EventID></System></Event>", "not xml at all"] {
            assert!(parse_health_xml(bad).is_none(), "{bad}");
        }
        // data inválida
        let bad_date = xml("Display", 4101, "").replace("2026-09-27T19:30:44.9640781Z", "yesterday");
        assert!(parse_health_xml(&bad_date).is_none());
    }

    #[test]
    fn out_of_range_numbers_are_dropped_not_truncated() {
        let r = rec("Microsoft-Windows-Kernel-Power", 41, "<Data Name='BugcheckCode'>99999999999</Data>").unwrap();
        assert_eq!(r.code, None);
        let r = rec("Microsoft-Windows-WindowsUpdateClient", 20, "<Data Name='errorCode'>0x1FFFFFFFF</Data>").unwrap();
        assert_eq!(r.code, None);
    }

    #[test]
    fn the_query_filter_lists_each_id_once() {
        let f = id_filter();
        let parts: Vec<&str> = f.split(" or ").collect();
        let mut unique = parts.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(parts.len(), unique.len(), "repeated id: {f}");
        for r in RULES {
            assert!(parts.contains(&format!("EventID={}", r.event_id).as_str()), "{}", r.event_id);
        }
    }
}
