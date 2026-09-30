//! Recorder Engine: gravação local cifrada, segmentada e com integridade verificável.

pub mod error;
pub mod keys;
pub mod recorder;
pub mod segment;

pub use error::{RecorderError, Result};
pub use keys::{KeyProvider, StaticKey};
#[cfg(windows)]
pub use keys::DpapiKeyStore;
pub use recorder::{
    RecorderConfig, Recorder, RecoveryReport, SegmentInfo, VerifyReport,
};
