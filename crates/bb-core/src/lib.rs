//! Núcleo do Developer Black Box: eventos, estados de gravação e Privacy Guard.

pub mod event;
pub mod exclusion;
pub mod guard;
pub mod state;

pub use event::{AcLine, EventKind, ExeName, ExeNameError, HealthCategory, HealthSample, InventoryItem, ProcessKey, ValidatedEvent};
pub use exclusion::{ExclusionKind, ExclusionSet};
pub use guard::{
    AuthError, Authorization, GuardConfig, Observation, PrivacyGuard, ProcessRef, MAX_AUTHORIZATION_MS,
    MIN_AUTHORIZATION_MS,
};
pub use state::{
    derive, PrivacyContext, ReasonCode, RecorderInputs, RecorderState, SensitiveReason,
};
