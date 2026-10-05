//! Leitura do log "Application" via API de Eventos do Windows (sem privilégios de administrador).
//!
//! Consulta só os eventos 1000 (falha) e 1002 (travamento) do log "Application", e os IDs de saúde da lista fixa
//! do log "System", a partir de um instante. O XML
//! de cada evento é interpretado por `eventlog::parse_event_xml` e descartado.

use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS};
use windows::Win32::System::EventLog::{
    EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryForwardDirection, EvtRender, EvtRenderEventXml, EVT_HANDLE,
};

use crate::eventlog::{iso_from_ms, parse_event_xml, CrashRecord, CrashSource};
use crate::healthlog::{id_filter, parse_health_xml, HealthRecord, HealthSource};
use crate::sample::CollectError;

/// Teto de registros por consulta, para uma rajada de falhas não travar o ciclo.
const MAX_RECORDS: usize = 500;

pub struct WindowsCrashSource;

impl WindowsCrashSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsCrashSource {
    fn default() -> Self {
        Self::new()
    }
}

struct Evt(EVT_HANDLE);
impl Drop for Evt {
    fn drop(&mut self) {
        // SAFETY: handle obtido de EvtQuery/EvtNext e fechado uma única vez.
        unsafe {
            let _ = EvtClose(self.0);
        }
    }
}

/// Consulta um canal e entrega o XML de cada evento a `on_xml`, até `MAX_RECORDS` aceitos. Sem administrador.
fn query_channel(channel: &HSTRING, xpath: &HSTRING, mut on_xml: impl FnMut(&str) -> bool) -> Result<(), CollectError> {
    let mut accepted = 0usize;
    // SAFETY: buffers locais; todos os handles são fechados por RAII.
    unsafe {
        let results = Evt(EvtQuery(None, channel, xpath, (EvtQueryChannelPath.0 | EvtQueryForwardDirection.0) as u32)
            .map_err(|e| CollectError(format!("EvtQuery: {e}")))?);
        loop {
            let mut handles = [0isize; 32];
            let mut returned = 0u32;
            match EvtNext(results.0, &mut handles, 1000, 0, &mut returned) {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_NO_MORE_ITEMS.to_hresult() => break,
                Err(e) => return Err(CollectError(format!("EvtNext: {e}"))),
            }
            for h in handles.iter().take(returned as usize) {
                let ev = Evt(EVT_HANDLE(*h));
                if let Some(xml) = render_xml(&ev) {
                    if on_xml(&xml) {
                        accepted += 1;
                    }
                }
            }
            if accepted >= MAX_RECORDS {
                break;
            }
        }
    }
    Ok(())
}

impl CrashSource for WindowsCrashSource {
    fn poll(&mut self, since_utc_ms: i64) -> Result<Vec<CrashRecord>, CollectError> {
        let query = HSTRING::from(format!(
            "*[System[(EventID=1000 or EventID=1002) and TimeCreated[@SystemTime>='{}']]]",
            iso_from_ms(since_utc_ms)
        ));
        let mut out = Vec::new();
        query_channel(&HSTRING::from("Application"), &query, |xml| match parse_event_xml(xml) {
            Some(rec) => {
                out.push(rec);
                true
            }
            None => false,
        })?;
        Ok(out)
    }
}

/// Eventos de saúde do log `System` (lista fixa de IDs em `healthlog::RULES`). Se o canal não puder ser lido,
/// devolve erro e o motor trata a fonte como indisponível.
pub struct WindowsHealthSource;

impl WindowsHealthSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsHealthSource {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthSource for WindowsHealthSource {
    fn poll(&mut self, since_utc_ms: i64) -> Result<Vec<HealthRecord>, CollectError> {
        let query = HSTRING::from(format!("*[System[({}) and TimeCreated[@SystemTime>='{}']]]", id_filter(), iso_from_ms(since_utc_ms)));
        let mut out = Vec::new();
        query_channel(&HSTRING::from("System"), &query, |xml| match parse_health_xml(xml) {
            Some(rec) => {
                out.push(rec);
                true
            }
            None => false,
        })?;
        Ok(out)
    }
}

/// # Safety
/// `ev` deve ser um handle de evento válido.
unsafe fn render_xml(ev: &Evt) -> Option<String> {
    let (mut used, mut props) = (0u32, 0u32);
    let first = EvtRender(None, ev.0, EvtRenderEventXml.0 as u32, 0, None, &mut used, &mut props);
    match first {
        Err(e) if e.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        _ => return None,
    }
    let mut buf = vec![0u16; (used as usize).div_ceil(2) + 1];
    EvtRender(None, ev.0, EvtRenderEventXml.0 as u32, (buf.len() * 2) as u32, Some(buf.as_mut_ptr().cast()), &mut used, &mut props).ok()?;
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}
