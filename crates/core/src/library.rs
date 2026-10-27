use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use picker_imaging::{
    dhash, extract, Decoder, ImageFormat, ImagingError, PlatformDecoder, RustFallbackDecoder,
    ThumbnailSize,
};
use picker_indexer::{IdentityReport, IdentityResolver};
use picker_store::{
    Asset, AssetFilter, AssetMetadata, Location, LocationStatus, NewRoot, NewTag, Root, RootStatus,
    Stage, StageStatus, StageTarget, Store, StoreError, Tag, TagAlias, TagAssignment, TagOrigin,
    TagState,
};

/// Errors returned by the application-independent library service.
#[derive(Debug)]
pub enum LibraryError {
    Store(StoreError),
    Identity(picker_indexer::IdentityError),
    Imaging(ImagingError),
    Io(std::io::Error),
    InvalidInput(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Identity(error) => error.fmt(formatter),
            Self::Imaging(error) => error.fmt(formatter),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::InvalidInput(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for LibraryError {}
impl From<StoreError> for LibraryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<picker_indexer::IdentityError> for LibraryError {
    fn from(value: picker_indexer::IdentityError) -> Self {
        Self::Identity(value)
    }
}
impl From<ImagingError> for LibraryError {
    fn from(value: ImagingError) -> Self {
        Self::Imaging(value)
    }
}
impl From<std::io::Error> for LibraryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// Thumbnail readiness exposed to the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailState {
    Missing,
    Ready,
    Failed,
}

/// One grid item.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetSummary {
    pub id: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
    pub captured_at: Option<i64>,
    pub thumbnail: ThumbnailState,
    pub location_status: LocationStatus,
}

/// A page of assets.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetPage {
    pub items: Vec<AssetSummary>,
    pub next_cursor: Option<i64>,
    pub total: u32,
    pub total_is_exact: bool,
}

/// One physical location of an asset.
#[derive(Debug, Clone, PartialEq)]
pub struct LocationView {
    pub id: i64,
    pub root_id: i64,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub status: LocationStatus,
}

/// A tag attached to an asset.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetTag {
    pub assignment: TagAssignment,
    pub tag: Tag,
}

/// Detail panel data.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetDetail {
    pub summary: AssetSummary,
    pub file_size: u64,
    pub locations: Vec<LocationView>,
    pub metadata: Option<AssetMetadata>,
    pub tags: Vec<AssetTag>,
}

/// Root information for the folder tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSummary {
    pub root: Root,
    pub image_count: u32,
}

/// Per-stage progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageProgress {
    pub stage: Stage,
    pub completed: u32,
    pub pending: u32,
    pub failed: u32,
}

/// Overall indexing status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStatus {
    pub stages: Vec<StageProgress>,
    pub paused: bool,
    pub total_assets: u32,
}

/// Application-independent library facade used by the Tauri layer.
#[derive(Debug)]
pub struct Library {
    store: Store,
    cache_root: PathBuf,
}

impl Library {
    /// Opens a database and prepares the thumbnail cache directory.
    pub fn open(database: impl AsRef<Path>, cache_root: impl Into<PathBuf>) -> Result<Self> {
        let cache_root = cache_root.into();
        fs::create_dir_all(&cache_root)?;
        Ok(Self {
            store: Store::open(database)?,
            cache_root,
        })
    }

    /// Opens an isolated in-memory library, used by tests.
    pub fn open_in_memory(cache_root: impl Into<PathBuf>) -> Result<Self> {
        let cache_root = cache_root.into();
        fs::create_dir_all(&cache_root)?;
        Ok(Self {
            store: Store::open_in_memory()?,
            cache_root,
        })
    }

    #[must_use]
    pub const fn store(&self) -> &Store {
        &self.store
    }

    #[must_use]
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub fn add_root(&self, path: impl AsRef<Path>, display_name: &str) -> Result<Root> {
        let path = path.as_ref();
        let absolute = path.to_str().ok_or_else(|| {
            LibraryError::InvalidInput("root path must be valid UTF-8".to_owned())
        })?;
        let metadata = fs::metadata(path)?;
        if !metadata.is_dir() {
            return Err(LibraryError::InvalidInput(
                "root path is not a directory".to_owned(),
            ));
        }
        let name = if display_name.trim().is_empty() {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(absolute)
        } else {
            display_name
        };
        Ok(self.store.roots().register(&NewRoot {
            absolute_path: absolute,
            volume_id: None,
            display_name: name,
            enabled: true,
            status: RootStatus::Online,
        })?)
    }

    pub fn list_roots(&self) -> Result<Vec<RootSummary>> {
        let roots = self.store.roots().list()?;
        let mut summaries = Vec::with_capacity(roots.len());
        for root in roots {
            let image_count = u32::try_from(self.store.locations().list_for_root(root.id)?.len())
                .unwrap_or(u32::MAX);
            summaries.push(RootSummary { root, image_count });
        }
        Ok(summaries)
    }

    pub fn remove_root(&self, root_id: i64) -> Result<bool> {
        Ok(self.store.roots().delete(root_id)?)
    }

    pub fn set_root_enabled(&self, root_id: i64, enabled: bool) -> Result<Root> {
        Ok(self.store.roots().set_enabled(root_id, enabled)?)
    }

    /// Synchronizes a root and processes newly discovered files.
    ///
    /// Thumbnail, metadata, and perceptual-hash failures are recorded per asset
    /// and do not stop the rest of the scan.
    pub fn scan_root(&self, root_id: i64) -> Result<IdentityReport> {
        let root = self.store.roots().get(root_id)?;
        let resolver = IdentityResolver::new(&self.store);
        let report = resolver.sync_root(root.id, &root.absolute_path)?;
        self.process_root(root.id)?;
        Ok(report)
    }

    fn process_root(&self, root_id: i64) -> Result<usize> {
        let root = self.store.roots().get(root_id)?;
        let mut processed = 0;
        for location in self.store.locations().list_for_root(root_id)? {
            if location.status != LocationStatus::Present {
                continue;
            }
            let source = Path::new(&root.absolute_path).join(&location.relative_path);
            let Some(format) = ImageFormat::from_path(&source) else {
                continue;
            };
            let already_processed = self
                .store
                .assets()
                .get(location.asset_id)?
                .perceptual_hash
                .is_some()
                && self
                    .thumbnail_path(location.asset_id, ThumbnailSize::Grid)
                    .is_file()
                && self
                    .thumbnail_path(location.asset_id, ThumbnailSize::Detail)
                    .is_file();
            if already_processed {
                continue;
            }
            match self.process_asset(location.asset_id, &source, format) {
                Ok(()) => processed += 1,
                Err(error) => {
                    let _ = self.store.stage_states().upsert(
                        StageTarget::Asset(location.asset_id),
                        Stage::Thumbnail,
                        StageStatus::Failed,
                        "thumbnail-v1",
                        Some(&error.to_string()),
                    );
                }
            }
        }
        Ok(processed)
    }

    fn process_asset(&self, asset_id: i64, source: &Path, format: ImageFormat) -> Result<()> {
        let limits = picker_imaging::DecodeLimits::default();
        let cache = picker_imaging::ThumbnailCache::new(&self.cache_root);
        let decoder = PlatformDecoder;
        cache.generate(&decoder, asset_id, source, format, &limits)?;
        let decoded = decoder.decode_file(source, format, &limits)?;
        self.store
            .assets()
            .set_perceptual_hash(asset_id, &dhash(&decoded))?;
        let metadata = extract(&RustFallbackDecoder, source, format, &limits)?;
        self.store.assets().set_metadata(&AssetMetadata {
            asset_id,
            camera_make: metadata.camera_make,
            camera_model: metadata.camera_model,
            lens: metadata.lens,
            exposure_time: metadata.exposure_time,
            f_number: metadata.f_number,
            iso: metadata.iso,
            focal_length_mm: metadata.focal_length_mm,
            gps_latitude: metadata.gps.map(|gps| gps.latitude),
            gps_longitude: metadata.gps.map(|gps| gps.longitude),
        })?;
        Ok(())
    }

    pub fn asset_page(&self, after_id: i64, limit: u32) -> Result<AssetPage> {
        let limit = limit.clamp(1, 500);
        let assets = self.store.assets().page(after_id, limit + 1)?;
        let has_more = assets.len() > limit as usize;
        let items = assets
            .into_iter()
            .take(limit as usize)
            .map(|asset| self.summary(asset))
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = has_more.then(|| items.last().map(|item| item.id)).flatten();
        Ok(AssetPage {
            items,
            next_cursor,
            total: self.store.assets().count()?,
            total_is_exact: true,
        })
    }

    pub fn asset_detail(&self, asset_id: i64) -> Result<AssetDetail> {
        let asset = self.store.assets().get(asset_id)?;
        let summary = self.summary(asset.clone())?;
        let mut locations = Vec::new();
        for location in self.store.locations().list_for_asset(asset_id)? {
            let root = self.store.roots().get(location.root_id)?;
            locations.push(LocationView {
                id: location.id,
                root_id: location.root_id,
                relative_path: location.relative_path.clone(),
                absolute_path: Path::new(&root.absolute_path).join(&location.relative_path),
                status: location.status,
            });
        }
        let mut tags = Vec::new();
        for assignment in self.store.tag_assignments().list_for_asset(asset_id)? {
            if let Ok(tag) = self.store.tags().get(assignment.tag_id) {
                tags.push(AssetTag { assignment, tag });
            }
        }
        Ok(AssetDetail {
            summary,
            file_size: asset.file_size,
            locations,
            metadata: self.store.assets().metadata(asset_id)?,
            tags,
        })
    }

    pub fn list_tags(&self) -> Result<Vec<picker_store::TagWithCount>> {
        Ok(self.store.tags().list_with_counts()?)
    }

    pub fn create_tag(
        &self,
        canonical_name: &str,
        display_name_ja: &str,
        parent_id: Option<i64>,
        color: Option<&str>,
    ) -> Result<Tag> {
        Ok(self.store.tags().create(&NewTag {
            namespace: "user",
            canonical_name,
            display_name_ja,
            display_name_en: None,
            parent_id,
            color,
            origin: TagOrigin::User,
        })?)
    }

    pub fn rename_tag(
        &self,
        tag_id: i64,
        canonical_name: &str,
        display_name_ja: &str,
    ) -> Result<Tag> {
        Ok(self
            .store
            .tags()
            .rename(tag_id, canonical_name, display_name_ja)?)
    }

    pub fn set_tag_parent(&self, tag_id: i64, parent_id: Option<i64>) -> Result<Tag> {
        Ok(self.store.tags().set_parent(tag_id, parent_id)?)
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<bool> {
        Ok(self.store.tags().delete(tag_id)?)
    }

    pub fn merge_tags(&self, source_id: i64, target_id: i64) -> Result<Tag> {
        Ok(self.store.tags().merge(source_id, target_id)?)
    }

    pub fn add_tag_alias(&self, tag_id: i64, alias: &str) -> Result<TagAlias> {
        Ok(self.store.tags().add_alias(tag_id, alias)?)
    }

    pub fn remove_tag_alias(&self, alias_id: i64) -> Result<bool> {
        Ok(self.store.tags().remove_alias(alias_id)?)
    }

    pub fn tag_aliases(&self, tag_id: i64) -> Result<Vec<TagAlias>> {
        Ok(self.store.tags().aliases(tag_id)?)
    }

    /// Applies or removes a confirmed tag on several assets at once.
    pub fn set_tag_on_assets(
        &self,
        asset_ids: &[i64],
        tag_id: i64,
        state: TagState,
    ) -> Result<usize> {
        Ok(self
            .store
            .tag_assignments()
            .set_user_many(asset_ids, tag_id, state)?)
    }

    pub fn remove_tag_from_assets(&self, asset_ids: &[i64], tag_id: i64) -> Result<usize> {
        Ok(self
            .store
            .tag_assignments()
            .remove_user_many(asset_ids, tag_id)?)
    }

    /// Applies tag, attribute, and cursor filters in the database.
    pub fn filter_page(
        &self,
        filter: &AssetFilter,
        after_id: i64,
        limit: u32,
    ) -> Result<AssetPage> {
        let limit = limit.clamp(1, 500);
        let assets = self
            .store
            .asset_filters()
            .page(filter, after_id, limit + 1)?;
        let has_more = assets.len() > limit as usize;
        let items = assets
            .into_iter()
            .take(limit as usize)
            .map(|asset| self.summary(asset))
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = has_more.then(|| items.last().map(|item| item.id)).flatten();
        Ok(AssetPage {
            items,
            next_cursor,
            total: self.store.asset_filters().count(filter)?,
            total_is_exact: true,
        })
    }

    /// Returns assets that have more than one location.
    pub fn exact_duplicate_assets(&self) -> Result<Vec<AssetSummary>> {
        self.store
            .assets()
            .exact_duplicates()?
            .into_iter()
            .map(|asset| self.summary(asset))
            .collect()
    }

    pub fn thumbnail_path(&self, asset_id: i64, size: ThumbnailSize) -> PathBuf {
        picker_imaging::thumbnail_path(&self.cache_root, asset_id, size.max_edge())
    }

    pub fn index_status(&self) -> Result<IndexStatus> {
        let mut stages = Vec::new();
        let counts = self.store.stage_states().counts()?;
        for stage in Stage::ordered() {
            let mut completed = 0;
            let mut pending = 0;
            let mut failed = 0;
            for (counted_stage, status, count) in &counts {
                if counted_stage != stage {
                    continue;
                }
                match status {
                    picker_store::StageStatus::Succeeded => completed += count,
                    picker_store::StageStatus::Failed => failed += count,
                    picker_store::StageStatus::Pending | picker_store::StageStatus::Running => {
                        pending += count;
                    }
                }
            }
            stages.push(StageProgress {
                stage: *stage,
                completed,
                pending,
                failed,
            });
        }
        let paused = self.store.jobs().has_paused()?;
        Ok(IndexStatus {
            stages,
            paused,
            total_assets: self.store.assets().count()?,
        })
    }

    /// Raises the priority of queued work for assets currently on screen.
    pub fn prioritize_viewport(&self, asset_ids: &[i64]) -> Result<usize> {
        Ok(self
            .store
            .jobs()
            .set_priority_for_assets(asset_ids, 1_000)?)
    }

    /// Pauses pending jobs; the worker loop observes the status before dequeuing.
    pub fn pause_index(&self) -> Result<usize> {
        Ok(self.store.jobs().pause_pending()?)
    }

    pub fn resume_index(&self) -> Result<usize> {
        Ok(self.store.jobs().resume_paused()?)
    }

    fn summary(&self, asset: Asset) -> Result<AssetSummary> {
        let locations = self.store.locations().list_for_asset(asset.id)?;
        let location_status = preferred_status(&locations);
        Ok(AssetSummary {
            id: asset.id,
            width: asset.width,
            height: asset.height,
            format: asset.format,
            captured_at: asset.captured_at,
            thumbnail: if self.thumbnail_path(asset.id, ThumbnailSize::Grid).is_file() {
                ThumbnailState::Ready
            } else {
                ThumbnailState::Missing
            },
            location_status,
        })
    }
}

fn preferred_status(locations: &[Location]) -> LocationStatus {
    if locations
        .iter()
        .any(|location| location.status == LocationStatus::Present)
    {
        LocationStatus::Present
    } else if locations
        .iter()
        .any(|location| location.status == LocationStatus::Offline)
    {
        LocationStatus::Offline
    } else {
        LocationStatus::Missing
    }
}
