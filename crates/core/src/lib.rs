#![deny(unsafe_code)]

//! Domain and use-case layer for the Picker application.

/// Entry point for application-independent core services.
///
/// The implementation is intentionally empty in T0-1. Later milestones add
/// domain models and use cases without coupling them to the Tauri app.
#[derive(Debug, Default)]
pub struct Core;

impl Core {
    /// Creates an empty core instance for the Tauri bootstrap layer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
