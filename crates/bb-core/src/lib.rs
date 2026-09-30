//! Núcleo do Developer Black Box: eventos, estados de gravação e Privacy Guard.

pub mod event;
pub mod guard;
pub mod state;

pub use event::{EventKind, ExeName, ExeNameError, ProcessKey, ValidatedEvent};
pub use guard::{
    AuthError, Authorization, GuardConfig, Observation, PrivacyGuard, MAX_AUTHORIZATION_MS, MIN_AUTHORIZATION_MS,
};
pub use state::{
    derive, PrivacyContext, ReasonCode, RecorderInputs, RecorderState, SensitiveReason,
};
