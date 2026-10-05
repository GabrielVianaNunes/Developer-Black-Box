//! Janela de leitura dos eventos de saúde: o que pode ser lido do Event Log `System` e o que nunca é reconstruído.
//!
//! Lógica pura (sem relógio, sem Windows, sem armazenamento), para ser testada com tempos sintéticos.
//!
//! Regras:
//! - Enquanto a saúde pode ser lida (não há pausa manual etc.) existe um intervalo ABERTO que começa no instante em
//!   que ela passou a poder ser lida. O que o Windows registrou antes disso, durante uma pausa, nunca é gravado depois.
//! - Ao parar de poder ler (pausa manual, modo restrito), o intervalo aberto é descartado: nada do tempo parado entra.
//! - Exceção explícita: o intervalo entre a última leitura de uma execução anterior (que terminou lendo) e o início
//!   desta (o app estava fechado, não pausado). É assim que um desligamento inesperado, registrado no boot seguinte,
//!   pode ser visto. Esse "atraso" tem teto de 7 dias.
//! - O mesmo evento (ID e número) repetido em menos de 60 s vira um só registro, o que também cobre a sobreposição entre
//!   consultas; há um teto por consulta.

use std::collections::HashMap;

use bb_collector::HealthRecord;

/// Intervalo mínimo entre duas consultas ao Event Log (relógio monotônico).
pub const POLL_INTERVAL_MS: u64 = 30_000;
/// Teto do "atraso" lido de uma execução anterior.
pub const BACKLOG_MAX_MS: i64 = 7 * 86_400_000;
/// O Windows grava o evento alguns instantes depois do fato; cada consulta recua isto para não perdê-lo.
const OVERLAP_MS: i64 = 3_000;
/// Mesmo evento (ID e número) repetido em menos que isto vira um só registro.
const COALESCE_MS: i64 = 60_000;
/// Teto de registros aceitos por consulta (uma rajada de erros de disco não pode encher o armazenamento).
pub const MAX_PER_POLL: usize = 50;

type EventKey = (u16, Option<u32>);

#[derive(Default)]
pub struct HealthWindow {
    /// Intervalos fechados ainda não lidos (atraso de uma execução anterior): [de, até).
    backlog: Vec<(i64, i64)>,
    /// Início do intervalo aberto, se a saúde pode ser lida agora.
    open_from: Option<i64>,
    last_accepted: HashMap<EventKey, i64>,
}

impl HealthWindow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Atraso a ler: a execução anterior terminou lendo em `cursor`; o app abriu agora (`now`). O teto de 7 dias vale
    /// aqui. Um cursor no futuro ou depois de `now` não gera nada.
    pub fn with_backlog(cursor: i64, now: i64) -> Self {
        let mut w = Self::default();
        let from = cursor.max(now.saturating_sub(BACKLOG_MAX_MS));
        if from < now {
            w.backlog.push((from, now));
        }
        w
    }

    pub fn is_active(&self) -> bool {
        self.open_from.is_some()
    }

    /// A saúde passou a poder ser lida em `now`. Idempotente.
    pub fn activate(&mut self, now: i64) {
        self.open_from.get_or_insert(now);
    }

    /// A saúde deixou de poder ser lida. Devolve se estava ativa. O tempo parado nunca entra depois.
    pub fn deactivate(&mut self) -> bool {
        self.last_accepted.clear();
        self.open_from.take().is_some()
    }

    /// A pessoa desligou a leitura: some o intervalo aberto E o atraso da execução anterior, para que ligar de novo
    /// comece de agora e nunca leia o período desligado. Devolve se havia algo a descartar.
    pub fn discard(&mut self) -> bool {
        let was_open = self.deactivate();
        let had_backlog = !self.backlog.is_empty();
        self.backlog.clear();
        was_open || had_backlog
    }

    /// Menor instante a pedir ao Event Log (`None` = nada a ler).
    pub fn since(&self) -> Option<i64> {
        self.backlog.iter().map(|(from, _)| *from).chain(self.open_from).min()
    }

    fn in_range(&self, ts: i64) -> bool {
        self.open_from.is_some_and(|from| ts >= from) || self.backlog.iter().any(|(from, to)| ts >= *from && ts < *to)
    }

    /// Filtra o que a fonte devolveu. Não confia na fonte: horário, repetição, rajada e teto são decididos aqui.
    pub fn accept(&mut self, mut records: Vec<HealthRecord>) -> Vec<HealthRecord> {
        records.sort_by_key(|r| r.ts_utc_ms);
        let mut out = Vec::new();
        for r in records {
            if out.len() >= MAX_PER_POLL {
                break;
            }
            if !self.in_range(r.ts_utc_ms) {
                continue; // fora de qualquer intervalo permitido (ex. durante uma pausa): nunca reconstruído
            }
            let key: EventKey = (r.event_id, r.code);
            if self.last_accepted.get(&key).is_some_and(|last| (r.ts_utc_ms - last).abs() < COALESCE_MS) {
                continue;
            }
            self.last_accepted.insert(key, r.ts_utc_ms);
            out.push(r);
        }
        out
    }

    /// Consulta concluída em `now`: o atraso foi lido e o intervalo aberto recua só a sobreposição.
    pub fn finish_poll(&mut self, now: i64) {
        self.backlog.retain(|(_, to)| *to > now);
        if let Some(from) = self.open_from.as_mut() {
            *from = (*from).max(now - OVERLAP_MS);
        }
        let newest = self.last_accepted.values().copied().max().unwrap_or(now);
        self.last_accepted.retain(|_, ts| newest - *ts < COALESCE_MS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::HealthCategory;

    fn rec(ts: i64, id: u16, code: Option<u32>) -> HealthRecord {
        HealthRecord { ts_utc_ms: ts, category: HealthCategory::DiskError, event_id: id, code }
    }

    #[test]
    fn nothing_is_read_before_the_window_opens() {
        let mut w = HealthWindow::new();
        assert_eq!(w.since(), None);
        assert!(!w.is_active());
        assert!(w.accept(vec![rec(5, 51, None)]).is_empty());
    }

    #[test]
    fn events_before_the_window_opened_are_never_accepted() {
        // pausa até 10_000; só depois disso o app pode ler
        let mut w = HealthWindow::new();
        w.activate(10_000);
        assert_eq!(w.since(), Some(10_000));
        let got = w.accept(vec![rec(9_999, 51, None), rec(10_000, 51, Some(1)), rec(12_000, 7, None)]);
        assert_eq!(got.iter().map(|r| r.ts_utc_ms).collect::<Vec<_>>(), vec![10_000, 12_000]);
    }

    #[test]
    fn a_pause_discards_the_open_window_and_the_time_in_between_is_lost_for_good() {
        let mut w = HealthWindow::new();
        w.activate(1_000);
        w.finish_poll(40_000);
        assert!(w.deactivate()); // pausa manual aos ~40 s
        assert!(!w.deactivate(), "second deactivate reports nothing active");
        assert_eq!(w.since(), None, "no polling while paused");
        w.activate(100_000); // retomada
        // evento da pausa (50_000) pedido por uma fonte mal comportada: rejeitado; o novo (100_500), aceito
        let got = w.accept(vec![rec(50_000, 51, None), rec(100_500, 51, None)]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].ts_utc_ms, 100_500);
    }

    #[test]
    fn activate_is_idempotent_and_does_not_move_the_start() {
        let mut w = HealthWindow::new();
        w.activate(1_000);
        w.activate(9_000);
        assert_eq!(w.since(), Some(1_000));
    }

    #[test]
    fn backlog_from_a_previous_run_is_read_once_and_only_that_range() {
        // execução anterior terminou lendo em 1_000; este app abriu em 100_000 (e a pausa começou lá)
        let mut w = HealthWindow::with_backlog(1_000, 100_000);
        assert_eq!(w.since(), Some(1_000));
        w.activate(500_000); // o usuário só retoma em 500_000
        let got = w.accept(vec![
            rec(2_000, 6008, None),   // no atraso: aceito (o app estava fechado)
            rec(300_000, 51, None),   // durante a pausa desta execução: rejeitado
            rec(500_001, 51, Some(2)), // depois de retomar: aceito
        ]);
        assert_eq!(got.iter().map(|r| r.ts_utc_ms).collect::<Vec<_>>(), vec![2_000, 500_001]);
        w.finish_poll(500_010);
        assert_eq!(w.since(), Some(500_000), "backlog is gone after being read; only the open window remains");
    }

    #[test]
    fn backlog_is_capped_at_seven_days() {
        let now = 40 * 86_400_000;
        let w = HealthWindow::with_backlog(0, now);
        assert_eq!(w.since(), Some(now - BACKLOG_MAX_MS));
    }

    #[test]
    fn a_cursor_in_the_future_or_equal_to_now_gives_no_backlog() {
        assert_eq!(HealthWindow::with_backlog(2_000, 1_000).since(), None);
        assert_eq!(HealthWindow::with_backlog(1_000, 1_000).since(), None);
    }

    #[test]
    fn a_failed_poll_keeps_the_backlog_for_the_next_try() {
        let mut w = HealthWindow::with_backlog(1_000, 100_000);
        w.activate(100_000);
        // sem finish_poll (a consulta falhou): nada mudou
        assert_eq!(w.since(), Some(1_000));
    }

    #[test]
    fn the_same_event_seen_twice_is_accepted_once() {
        let mut w = HealthWindow::new();
        w.activate(0);
        assert_eq!(w.accept(vec![rec(1_000, 7, None)]).len(), 1);
        w.finish_poll(2_000);
        // a consulta seguinte (com sobreposição) devolve o mesmo evento de novo
        assert!(w.accept(vec![rec(1_000, 7, None)]).is_empty());
    }

    #[test]
    fn identical_bursts_are_condensed_but_different_events_are_not() {
        let mut w = HealthWindow::new();
        w.activate(0);
        let burst: Vec<HealthRecord> = (0..30).map(|i| rec(1_000 + i * 100, 51, None)).collect();
        assert_eq!(w.accept(burst).len(), 1);
        let got = w.accept(vec![rec(5_000, 7, None), rec(5_001, 51, Some(9))]);
        assert_eq!(got.len(), 2, "other id / other code are different events");
        // depois da janela de condensação o mesmo evento volta a valer
        assert_eq!(w.accept(vec![rec(62_000, 51, None)]).len(), 1);
    }

    #[test]
    fn a_poll_never_returns_more_than_the_cap() {
        let mut w = HealthWindow::new();
        w.activate(0);
        let many: Vec<HealthRecord> = (0..200u16).map(|i| rec(1_000 + i64::from(i), 1000 + i, None)).collect();
        assert_eq!(w.accept(many).len(), MAX_PER_POLL);
    }

    #[test]
    fn deactivating_forgets_the_dedup_memory() {
        let mut w = HealthWindow::new();
        w.activate(0);
        assert_eq!(w.accept(vec![rec(1_000, 7, None)]).len(), 1);
        w.deactivate();
        w.activate(0);
        assert_eq!(w.accept(vec![rec(1_000, 7, None)]).len(), 1, "memory does not leak across a pause");
    }
}
