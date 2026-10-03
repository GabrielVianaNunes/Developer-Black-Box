//! Collector: amostras de processos e sinais de contexto, sem conteúdo.

pub mod apps;
pub mod differ;
pub mod eventlog;
pub mod healthlog;
pub mod hosted;
pub mod inventory;
pub mod lnk;
pub mod power;
pub mod sample;
pub mod telemetry;
#[cfg(windows)]
pub mod eventlog_win;
#[cfg(windows)]
pub mod inventory_win;
#[cfg(windows)]
pub mod pdh_win;
#[cfg(windows)]
pub mod power_win;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
pub mod windows_impl;

pub use differ::{Differ, MetricsConfig};
pub use eventlog::{CrashKind, CrashRecord, CrashSource};
pub use healthlog::{HealthRecord, HealthSource};
pub use inventory::{InventorySource, Snapshot};
pub use power::{PowerReading, PowerSource};
pub use telemetry::{RawCounters, SampleBuilder, TelemetrySource};
pub use sample::{CollectError, ContextSource, ProcessSample, ProcessSource, SystemSample};
#[cfg(windows)]
pub use eventlog_win::{WindowsCrashSource, WindowsHealthSource};
#[cfg(windows)]
pub use inventory_win::WindowsInventorySource;
#[cfg(windows)]
pub use pdh_win::WindowsTelemetrySource;
#[cfg(windows)]
pub use power_win::WindowsPowerSource;
#[cfg(windows)]
pub use windows_impl::{current_foreground_exe, WindowsContextSource, WindowsProcessSource};
