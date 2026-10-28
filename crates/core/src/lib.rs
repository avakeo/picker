#![deny(unsafe_code)]

//! Domain and use-case layer for the Picker application.

mod auto_album;
mod library;
mod palette;
mod portable;
mod worker;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod worker_tests;

pub use auto_album::{default_auto_albums, AlbumQuery, TagRef};
pub use library::{
    AssetDetail, AssetPage, AssetSummary, AssetTag, IndexStatus, Library, LibraryError,
    LocationView, Result, RootSummary, StageProgress, ThumbnailState, PORTABLE_EXPORT_VERSION,
};
pub use palette::{ColorCondition, ColorScope, Palette, PaletteColor};
pub use portable::{PortableAlbum, PortableColor, PortableColors, PortableFeedback, PortableRoot};
pub use worker::{
    parse_pmset, BatteryPolicy, JobRun, PoolConfig, PowerSource, SystemPower, WorkerPool,
    WorkerSettings, BATTERY_POLICY_KEY, DEFAULT_IMAGE_WORKERS, IMAGE_STAGES, IMAGE_WORKERS_KEY,
    MAX_IMAGE_WORKERS,
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
