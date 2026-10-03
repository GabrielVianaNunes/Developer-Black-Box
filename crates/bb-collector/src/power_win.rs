//! Energia no Windows: `GetSystemPowerStatus` (sem administrador, sem localização, sem rede).
//! Só os três bytes de estado são usados; a decisão está em `power::reading_from_raw`.

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use crate::power::{reading_from_raw, PowerReading, PowerSource};
use crate::sample::CollectError;

pub struct WindowsPowerSource;

impl WindowsPowerSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsPowerSource {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerSource for WindowsPowerSource {
    fn read(&mut self) -> Result<Option<PowerReading>, CollectError> {
        let mut st = SYSTEM_POWER_STATUS::default();
        // SAFETY: `st` é um destino válido para a estrutura.
        unsafe { GetSystemPowerStatus(&mut st) }.map_err(|e| CollectError(format!("GetSystemPowerStatus: {e}")))?;
        Ok(reading_from_raw(st.ACLineStatus, st.BatteryFlag, st.BatteryLifePercent))
    }
}
