#![deny(unsafe_code)]

//! Domain and use-case layer for the Picker application.

mod library;

#[cfg(test)]
mod tests;

pub use library::{
    AssetDetail, AssetPage, AssetSummary, IndexStatus, Library, LibraryError, LocationView, Result,
    RootSummary, StageProgress, ThumbnailState,
};

/// Entry point for application-independent core services.
#[derive(Debug, Default)]
pub struct Core;

impl Core {
    /// Creates an empty core instance for the Tauri bootstrap layer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
