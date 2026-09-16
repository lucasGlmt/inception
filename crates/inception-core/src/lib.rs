//! Pure, deterministic domain types and algorithms shared across the
//! Inception runtime. No USB, MIDI, filesystem, networking or compiler
//! dependencies — see `AGENTS.md`.

pub mod attribute;
pub mod color;
pub mod ids;
pub mod intensity;
pub mod lighting_state;
pub mod time;
pub mod transition;

pub use attribute::{Attribute, AttributeValue};
pub use color::Rgb;
pub use ids::{FixtureId, TargetId, UniverseId};
pub use intensity::Intensity;
pub use lighting_state::{LightingError, LightingState, ResolvedTarget};
pub use time::{Clock, Duration, MonotonicClock, Timestamp, VirtualClock};
pub use transition::{ActiveTransition, TransitionEngine, TransitionError};
