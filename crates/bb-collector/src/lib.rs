//! Collector: amostras de processos e sinais de contexto, sem conteúdo.

pub mod apps;
pub mod differ;
pub mod eventlog;
#[cfg(windows)]
pub mod eventlog_win;
pub mod healthlog;
pub mod hosted;
pub mod inventory;
#[cfg(windows)]
pub mod inventory_win;
pub mod lnk;
#[cfg(windows)]
pub mod pdh_win;
pub mod power;
#[cfg(windows)]
pub mod power_win;
pub mod sample;
#[cfg(windows)]
pub mod startup;
pub mod telemetry;
#[cfg(windows)]
pub mod windows_impl;

pub use differ::{Differ, MetricsConfig};
pub use eventlog::{CrashKind, CrashRecord, CrashSource};
#[cfg(windows)]
pub use eventlog_win::{WindowsCrashSource, WindowsHealthSource};
pub use healthlog::{HealthRecord, HealthSource};
pub use inventory::{InventorySource, Snapshot};
#[cfg(windows)]
pub use inventory_win::WindowsInventorySource;
#[cfg(windows)]
pub use pdh_win::WindowsTelemetrySource;
pub use power::{PowerReading, PowerSource};
#[cfg(windows)]
pub use power_win::WindowsPowerSource;
pub use sample::{CollectError, ContextSource, ProcessSample, ProcessSource, SystemSample};
pub use telemetry::{RawCounters, SampleBuilder, TelemetrySource};
#[cfg(windows)]
pub use windows_impl::{current_foreground_exe, WindowsContextSource, WindowsProcessSource};
