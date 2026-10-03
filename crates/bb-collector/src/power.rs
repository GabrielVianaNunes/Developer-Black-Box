//! Energia e bateria: tomada ligada/desligada e porcentagem de carga.
//!
//! Só dois estados e uma porcentagem. Nada de localização, rede, nome da bateria, fabricante ou número de série.
//! Puro (sem Windows) para ser testado com leituras sintéticas.

use bb_core::AcLine;

use crate::sample::CollectError;

/// Uma leitura da bateria. Computador sem bateria não tem leitura (a fonte fica "indisponível").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerReading {
    pub ac: Option<AcLine>,
    pub charge_percent: Option<u8>,
}

/// Fonte de energia. `Ok(None)` = sem bateria (ou não identificável); `Err` = falha de leitura. Os dois são
/// "indisponível" para o motor, sem erro e sem alarme.
pub trait PowerSource {
    fn read(&mut self) -> Result<Option<PowerReading>, CollectError>;
}

/// Converte os três bytes de `GetSystemPowerStatus` (ACLineStatus, BatteryFlag, BatteryLifePercent).
/// Qualquer valor fora do documentado vira "desconhecido" (`None`), nunca um número inventado.
/// - ACLineStatus: 0 = desligada, 1 = ligada, 255 = desconhecida.
/// - BatteryFlag: bit 128 = sem bateria; 255 = desconhecido.
/// - BatteryLifePercent: 0 a 100; 255 = desconhecido.
/// Sem bateria ou com flag desconhecida (255 também tem o bit 128): `None` (fonte indisponível).
pub fn reading_from_raw(ac_line_status: u8, battery_flag: u8, life_percent: u8) -> Option<PowerReading> {
    if battery_flag & 128 != 0 {
        return None;
    }
    let ac = match ac_line_status {
        0 => Some(AcLine::Offline),
        1 => Some(AcLine::Online),
        _ => None,
    };
    let charge_percent = (life_percent <= 100).then_some(life_percent);
    Some(PowerReading { ac, charge_percent })
}

/// Carga mínima de variação para gravar uma nova amostra (em pontos percentuais).
pub const CHARGE_STEP: u8 = 5;

/// Decide quando uma leitura merece virar evento, para a gravação não crescer com uma amostra por minuto.
/// Grava: a primeira leitura, qualquer mudança de tomada, qualquer mudança entre "carga conhecida" e "desconhecida",
/// e variações de carga de `CHARGE_STEP` pontos ou mais desde a última gravada.
#[derive(Debug, Default)]
pub struct PowerTracker {
    last: Option<PowerReading>,
}

impl PowerTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Esquece a última gravada: a próxima leitura volta a ser gravada (usado depois de uma pausa).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// `Some(leitura)` se deve gravar.
    pub fn decide(&mut self, now: PowerReading) -> Option<PowerReading> {
        let record = match self.last {
            None => true,
            Some(prev) => {
                prev.ac != now.ac
                    || match (prev.charge_percent, now.charge_percent) {
                        (Some(a), Some(b)) => a.abs_diff(b) >= CHARGE_STEP,
                        (None, None) => false,
                        _ => true,
                    }
            }
        };
        if record {
            self.last = Some(now);
            Some(now)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(ac: Option<AcLine>, p: Option<u8>) -> PowerReading {
        PowerReading { ac, charge_percent: p }
    }

    #[test]
    fn a_laptop_on_battery_and_on_the_charger() {
        assert_eq!(reading_from_raw(0, 8, 57), Some(r(Some(AcLine::Offline), Some(57))));
        assert_eq!(reading_from_raw(1, 9, 100), Some(r(Some(AcLine::Online), Some(100))));
        assert_eq!(reading_from_raw(1, 1, 0), Some(r(Some(AcLine::Online), Some(0))));
    }

    #[test]
    fn a_desktop_without_a_battery_is_unavailable() {
        assert_eq!(reading_from_raw(1, 128, 255), None);
        assert_eq!(reading_from_raw(1, 128 | 1, 100), None, "the no-battery bit wins over the others");
    }

    #[test]
    fn an_unknown_flag_is_unavailable_and_unknown_fields_are_none() {
        assert_eq!(reading_from_raw(1, 255, 50), None);
        assert_eq!(reading_from_raw(255, 1, 50), Some(r(None, Some(50))));
        assert_eq!(reading_from_raw(0, 1, 255), Some(r(Some(AcLine::Offline), None)));
        assert_eq!(reading_from_raw(7, 1, 101), Some(r(None, None)), "out-of-range values are never invented");
    }

    #[test]
    fn the_first_reading_is_recorded() {
        let mut t = PowerTracker::new();
        assert!(t.decide(r(Some(AcLine::Online), Some(100))).is_some());
    }

    #[test]
    fn small_charge_changes_are_not_recorded_but_a_step_is() {
        let mut t = PowerTracker::new();
        t.decide(r(Some(AcLine::Offline), Some(80)));
        for p in [79, 78, 77, 76] {
            assert!(t.decide(r(Some(AcLine::Offline), Some(p))).is_none(), "{p}");
        }
        assert!(t.decide(r(Some(AcLine::Offline), Some(75))).is_some(), "5 points since the last recorded one");
        assert!(t.decide(r(Some(AcLine::Offline), Some(72))).is_none(), "the reference moved to 75");
    }

    #[test]
    fn plugging_or_unplugging_is_always_recorded() {
        let mut t = PowerTracker::new();
        t.decide(r(Some(AcLine::Offline), Some(50)));
        assert!(t.decide(r(Some(AcLine::Online), Some(50))).is_some());
        assert!(t.decide(r(Some(AcLine::Online), Some(50))).is_none(), "same again");
        assert!(t.decide(r(Some(AcLine::Offline), Some(50))).is_some());
        assert!(t.decide(r(None, Some(50))).is_some(), "ac unknown is a change too");
    }

    #[test]
    fn charge_going_from_known_to_unknown_is_recorded_once() {
        let mut t = PowerTracker::new();
        t.decide(r(Some(AcLine::Online), Some(50)));
        assert!(t.decide(r(Some(AcLine::Online), None)).is_some());
        assert!(t.decide(r(Some(AcLine::Online), None)).is_none());
        assert!(t.decide(r(Some(AcLine::Online), Some(50))).is_some());
    }

    #[test]
    fn reset_makes_the_next_reading_recordable_again() {
        let mut t = PowerTracker::new();
        t.decide(r(Some(AcLine::Online), Some(50)));
        assert!(t.decide(r(Some(AcLine::Online), Some(50))).is_none());
        t.reset();
        assert!(t.decide(r(Some(AcLine::Online), Some(50))).is_some());
    }
}
