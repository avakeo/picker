#![deny(unsafe_code)]

//! Domain and use-case layer for the Picker application.

mod auto_album;
mod cache;
mod library;
mod ml_jobs;
mod palette;
mod portable;
mod watch;
mod worker;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod watch_tests;
#[cfg(test)]
mod worker_tests;

pub use auto_album::{default_auto_albums, AlbumQuery, TagRef};
pub use cache::{CacheClearReport, CacheTarget};
pub use library::{
    AssetDetail, AssetPage, AssetSummary, AssetTag, IndexStatus, Library, LibraryError,
    LocationView, Result, RootSummary, StageProgress, ThumbnailState, PORTABLE_EXPORT_VERSION,
};
pub use ml_jobs::ML_STAGES;
pub use palette::{ColorCondition, ColorScope, Palette, PaletteColor};
pub use portable::{PortableAlbum, PortableColor, PortableColors, PortableFeedback, PortableRoot};
pub use watch::{WatchConfig, WatchService};
pub use worker::{
    parse_pmset, BatteryPolicy, JobRun, PoolConfig, PowerSource, SystemPower, WorkerPool,
    WorkerSettings, BATTERY_POLICY_KEY, DEFAULT_IMAGE_WORKERS, DEFAULT_ML_WORKERS, IMAGE_STAGES,
    IMAGE_WORKERS_KEY, MAX_IMAGE_WORKERS, MAX_ML_WORKERS, ML_WORKERS_KEY,
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
