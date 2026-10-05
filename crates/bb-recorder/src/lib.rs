//! Recorder Engine: gravação local cifrada, segmentada e com integridade verificável.

pub mod error;
pub mod keys;
pub mod recorder;
pub mod segment;

pub use error::{RecorderError, Result};
#[cfg(windows)]
pub use keys::DpapiKeyStore;
pub use keys::{KeyProvider, StaticKey};
pub use recorder::{Recorder, RecorderConfig, RecoveryReport, SegmentInfo, VerifyReport};
