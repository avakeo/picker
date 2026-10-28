use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use picker_imaging::{
    dhash, extract, extract_colors, Decoder, ImageFormat, ImagingError, PlatformDecoder,
    RustFallbackDecoder, ThumbnailSize,
};
use picker_indexer::{IdentityReport, IdentityResolver};
use picker_ml::{
    select_review_candidates, BruteForceIndex, Embedder, LogisticRegression, Tagger, VectorIndex,
    ZeroShotTagger,
};
use picker_store::{
    Asset, AssetColor, AssetFilter, AssetMetadata, FeedbackAction, FeedbackEvent, Location,
    LocationStatus, NewRoot, NewTag, Root, RootStatus, Stage, StageStatus, StageTarget, Store,
    StoreError, Tag, TagAlias, TagAssignment, TagOrigin, TagState,
};
use serde::{Deserialize, Serialize};

use crate::portable::{PortableAlbum, PortableColors, PortableFeedback, PortableRoot};

/// Version recorded for the image stages (metadata, thumbnail, phash, color).
pub(crate) const IMAGE_STAGE_VERSION: &str = "thumbnail-v1";
/// Version recorded for the zero-shot auto-tag stage.
pub(crate) const AUTO_TAG_STAGE_VERSION: &str = "zero-shot-v1";

/// Portable representation of the non-regenerable tag data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableExport {
    pub version: u32,
    pub tags: Vec<PortableTag>,
    pub aliases: Vec<PortableAlias>,
    pub classifiers: Vec<PortableClassifier>,
    pub assignments: Vec<PortableAssignmentRecord>,
    /// Added in version 2. Version 1 files have none of the fields below.
    #[serde(default)]
    pub roots: Vec<PortableRoot>,
    #[serde(default)]
    pub albums: Vec<PortableAlbum>,
    #[serde(default)]
    pub user_colors: Vec<PortableColors>,
    #[serde(default)]
    pub feedback_events: Vec<PortableFeedback>,
}

/// Current version of [`PortableExport`]. Version 1 is still accepted.
pub const PORTABLE_EXPORT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableTag {
    pub namespace: String,
    pub canonical_name: String,
    pub display_name_ja: String,
    pub display_name_en: Option<String>,
    pub parent_namespace: Option<String>,
    pub parent_canonical_name: Option<String>,
    pub color: Option<String>,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableAlias {
    pub namespace: String,
    pub canonical_name: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableClassifier {
    pub namespace: String,
    pub canonical_name: String,
    pub dimensions: u32,
    pub weights: Vec<f64>,
    pub bias: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableAssignmentRecord {
    pub blake3_hash: String,
    pub namespace: String,
    pub canonical_name: String,
    pub state: String,
    pub source: String,
    pub confidence: Option<f64>,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
}

pub(crate) fn hash_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn parse_hash(value: &str) -> Result<Vec<u8>> {
    if value.len() != 64 {
        return Err(LibraryError::InvalidInput("invalid BLAKE3 hash".to_owned()));
    }
    let mut bytes = Vec::with_capacity(32);
    for index in (0..64).step_by(2) {
        bytes.push(
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| LibraryError::InvalidInput("invalid BLAKE3 hash".to_owned()))?,
        );
    }
    Ok(bytes)
}

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
    pub colors: Vec<AssetColor>,
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

/// Progress of the active embedding model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProgress {
    pub model_id: String,
    pub model_version: String,
    pub dimensions: u32,
    pub indexed: u32,
    pub missing: u32,
}

/// Overall indexing status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStatus {
    pub stages: Vec<StageProgress>,
    pub paused: bool,
    pub total_assets: u32,
}

/// Application-independent library facade used by the Tauri layer.
pub struct Library {
    store: Store,
    cache_root: PathBuf,
    embedder: Mutex<Option<Arc<dyn Embedder>>>,
    vectors: Mutex<Option<VectorState>>,
    tagger: Mutex<Option<Arc<TaggerState>>>,
}

struct VectorState {
    model_id: String,
    model_version: String,
    index: BruteForceIndex,
    /// Unix seconds of the last [`Library::sync_vectors`] read.
    synced_at: i64,
}

struct TaggerState {
    model_id: String,
    model_version: String,
    tagger: ZeroShotTagger,
    tag_ids: std::collections::HashMap<String, i64>,
}

impl fmt::Debug for Library {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Library")
            .field("store", &self.store)
            .field("cache_root", &self.cache_root)
            .field(
                "embedder",
                &self.embedder.lock().map(|value| value.is_some()),
            )
            .finish_non_exhaustive()
    }
}

impl Library {
    /// Opens a database and prepares the thumbnail cache directory.
    pub fn open(database: impl AsRef<Path>, cache_root: impl Into<PathBuf>) -> Result<Self> {
        let cache_root = cache_root.into();
        fs::create_dir_all(&cache_root)?;
        Ok(Self {
            store: Store::open(database)?,
            cache_root,
            embedder: Mutex::new(None),
            vectors: Mutex::new(None),
            tagger: Mutex::new(None),
        })
    }

    /// Opens an isolated in-memory library, used by tests.
    pub fn open_in_memory(cache_root: impl Into<PathBuf>) -> Result<Self> {
        let cache_root = cache_root.into();
        fs::create_dir_all(&cache_root)?;
        Ok(Self {
            store: Store::open_in_memory()?,
            cache_root,
            embedder: Mutex::new(None),
            vectors: Mutex::new(None),
            tagger: Mutex::new(None),
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

    /// Synchronizes a root and processes its queued image work before
    /// returning. The desktop app uses [`Library::sync_root`] and leaves the
    /// queue to the background workers instead.
    ///
    /// Thumbnail, metadata, and perceptual-hash failures are recorded per asset
    /// and do not stop the rest of the scan.
    pub fn scan_root(&self, root_id: i64) -> Result<IdentityReport> {
        let report = self.sync_root(root_id)?;
        self.run_queued_image_jobs()?;
        Ok(report)
    }

    /// Synchronizes a root with the file system and queues image work for
    /// files that still need it. Decisions waiting in a portable import are
    /// applied here, because they only need the asset to exist.
    pub fn sync_root(&self, root_id: i64) -> Result<IdentityReport> {
        let root = self.store.roots().get(root_id)?;
        let resolver = IdentityResolver::new(&self.store);
        let report = resolver.sync_root(root.id, &root.absolute_path)?;
        self.enqueue_unprocessed(root.id)?;
        for location in self.store.locations().list_for_root(root.id)? {
            if location.status == LocationStatus::Present {
                let asset = self.store.assets().get(location.asset_id)?;
                self.store
                    .portable()
                    .apply_for_asset(asset.id, &asset.blake3_hash)?;
                self.apply_pending_items(asset.id, &asset.blake3_hash)?;
            }
        }
        Ok(report)
    }

    /// Writes confirmed tags to an XMP sidecar when enabled in settings.
    pub fn export_xmp(&self, asset_id: i64) -> Result<usize> {
        if self
            .store
            .settings()
            .get("xmp.enabled")?
            .is_none_or(|setting| setting.value != "true")
        {
            return Ok(0);
        }
        let Some(source) = self.present_source(asset_id)? else {
            return Ok(0);
        };
        let detail = self.asset_detail(asset_id)?;
        let mut keywords = Vec::new();
        let mut hierarchical = Vec::new();
        for assigned in detail.tags {
            if assigned.assignment.state != TagState::Confirmed {
                continue;
            }
            keywords.push(assigned.tag.display_name_ja.clone());
            let mut chain = vec![assigned.tag.display_name_ja.clone()];
            let mut parent = assigned.tag.parent_id;
            while let Some(parent_id) = parent {
                let tag = self.store.tags().get(parent_id)?;
                chain.push(tag.display_name_ja);
                parent = tag.parent_id;
            }
            chain.reverse();
            hierarchical.push(chain.join("|"));
        }
        let sidecar = PathBuf::from(format!("{}.xmp", source.to_string_lossy()));
        let existing = fs::read_to_string(&sidecar).ok();
        let merged = picker_sidecar::merge(existing.as_deref(), &keywords, &hierarchical)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        fs::write(&sidecar, merged)?;
        Ok(keywords.len())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        crate::worker::validate_setting(key, value)?;
        self.store.settings().set(key, value)?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.store.settings().get(key)?.map(|setting| setting.value))
    }

    /// Writes portable tag data as JSON.
    pub fn export_portable(&self, path: &Path) -> Result<usize> {
        let tags = self.list_tags()?;
        let mut portable_tags = Vec::with_capacity(tags.len());
        let mut aliases = Vec::new();
        for entry in &tags {
            let parent = entry
                .tag
                .parent_id
                .and_then(|parent_id| tags.iter().find(|candidate| candidate.tag.id == parent_id));
            portable_tags.push(PortableTag {
                namespace: entry.tag.namespace.clone(),
                canonical_name: entry.tag.canonical_name.clone(),
                display_name_ja: entry.tag.display_name_ja.clone(),
                display_name_en: entry.tag.display_name_en.clone(),
                parent_namespace: parent.map(|parent| parent.tag.namespace.clone()),
                parent_canonical_name: parent.map(|parent| parent.tag.canonical_name.clone()),
                color: entry.tag.color.clone(),
                origin: entry.tag.origin.as_str().to_owned(),
            });
            for alias in self.tag_aliases(entry.tag.id)? {
                aliases.push(PortableAlias {
                    namespace: entry.tag.namespace.clone(),
                    canonical_name: entry.tag.canonical_name.clone(),
                    alias: alias.alias,
                });
            }
        }
        let mut classifiers = Vec::new();
        for entry in &tags {
            if let Some(classifier) = self.store.classifiers().get(entry.tag.id)? {
                classifiers.push(PortableClassifier {
                    namespace: entry.tag.namespace.clone(),
                    canonical_name: entry.tag.canonical_name.clone(),
                    dimensions: classifier.dimensions,
                    weights: classifier.weights,
                    bias: classifier.bias,
                });
            }
        }
        let assignments = self
            .store
            .portable()
            .decided_assignments()?
            .into_iter()
            .map(|assignment| PortableAssignmentRecord {
                blake3_hash: hash_hex(&assignment.blake3_hash),
                namespace: assignment.namespace,
                canonical_name: assignment.canonical_name,
                state: assignment.state.as_str().to_owned(),
                source: assignment.source.as_str().to_owned(),
                confidence: assignment.confidence,
                model_id: assignment.model_id,
                model_version: assignment.model_version,
            })
            .collect();
        let extras = self.export_extras()?;
        let export = PortableExport {
            version: PORTABLE_EXPORT_VERSION,
            tags: portable_tags,
            aliases,
            classifiers,
            assignments,
            roots: extras.roots,
            albums: extras.albums,
            user_colors: extras.colors,
            feedback_events: extras.feedback,
        };
        let json = serde_json::to_vec_pretty(&export)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        fs::write(path, json)?;
        Ok(export.tags.len())
    }

    /// Imports portable tag data. Assignments wait for matching content hashes.
    pub fn import_portable(&self, path: &Path) -> Result<usize> {
        let bytes = fs::read(path)?;
        let export: PortableExport = serde_json::from_slice(&bytes)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        if !(1..=PORTABLE_EXPORT_VERSION).contains(&export.version) {
            return Err(LibraryError::InvalidInput(
                "unsupported export version".to_owned(),
            ));
        }
        for tag in &export.tags {
            if self
                .store
                .tags()
                .find(&tag.namespace, &tag.canonical_name)?
                .is_none()
            {
                let origin: TagOrigin = tag.origin.parse().unwrap_or(TagOrigin::User);
                self.store.tags().create(&NewTag {
                    namespace: &tag.namespace,
                    canonical_name: &tag.canonical_name,
                    display_name_ja: &tag.display_name_ja,
                    display_name_en: tag.display_name_en.as_deref(),
                    parent_id: None,
                    color: tag.color.as_deref(),
                    origin,
                })?;
            }
        }
        for tag in &export.tags {
            if let (Some(namespace), Some(canonical)) =
                (&tag.parent_namespace, &tag.parent_canonical_name)
            {
                let child = self
                    .store
                    .tags()
                    .find(&tag.namespace, &tag.canonical_name)?;
                let parent = self.store.tags().find(namespace, canonical)?;
                if let (Some(child), Some(parent)) = (child, parent) {
                    if child.parent_id.is_none() && child.id != parent.id {
                        self.store.tags().set_parent(child.id, Some(parent.id))?;
                    }
                }
            }
        }
        for alias in &export.aliases {
            if let Some(tag) = self
                .store
                .tags()
                .find(&alias.namespace, &alias.canonical_name)?
            {
                let _ = self.store.tags().add_alias(tag.id, &alias.alias);
            }
        }
        for classifier in &export.classifiers {
            if let Some(tag) = self
                .store
                .tags()
                .find(&classifier.namespace, &classifier.canonical_name)?
            {
                self.store.classifiers().upsert(
                    tag.id,
                    classifier.dimensions,
                    &classifier.weights,
                    classifier.bias,
                )?;
            }
        }
        let mut imported = 0;
        for assignment in &export.assignments {
            let Some(tag) = self
                .store
                .tags()
                .find(&assignment.namespace, &assignment.canonical_name)?
            else {
                continue;
            };
            let hash = parse_hash(&assignment.blake3_hash)?;
            let state: TagState = assignment
                .state
                .parse()
                .map_err(|_| LibraryError::InvalidInput("invalid assignment state".to_owned()))?;
            let source: picker_store::AssignmentSource = assignment
                .source
                .parse()
                .map_err(|_| LibraryError::InvalidInput("invalid assignment source".to_owned()))?;
            self.store.portable().stage_assignment(
                &hash,
                tag.id,
                state,
                source,
                assignment.confidence,
                assignment.model_id.as_deref(),
                assignment.model_version.as_deref(),
            )?;
            imported += 1;
        }
        self.store.portable().apply_all()?;
        self.import_extras(
            &export.roots,
            &export.albums,
            &export.user_colors,
            &export.feedback_events,
        )?;
        Ok(imported)
    }

    /// Queues image work for every present file of a root that lacks a
    /// perceptual hash or a thumbnail, including files whose cache was removed.
    pub(crate) fn enqueue_unprocessed(&self, root_id: i64) -> Result<usize> {
        let mut queued = 0;
        for location in self.store.locations().list_for_root(root_id)? {
            if location.status != LocationStatus::Present
                || self.is_image_processed(location.asset_id)?
            {
                continue;
            }
            let target = StageTarget::Asset(location.asset_id);
            self.store.jobs().enqueue(target, Stage::Metadata, 0, 3)?;
            self.store.stage_states().upsert(
                target,
                Stage::Thumbnail,
                StageStatus::Pending,
                IMAGE_STAGE_VERSION,
                None,
            )?;
            queued += 1;
        }
        Ok(queued)
    }

    pub(crate) fn is_image_processed(&self, asset_id: i64) -> Result<bool> {
        Ok(self.store.assets().get(asset_id)?.perceptual_hash.is_some()
            && self.thumbnail_path(asset_id, ThumbnailSize::Grid).is_file()
            && self
                .thumbnail_path(asset_id, ThumbnailSize::Detail)
                .is_file())
    }

    pub(crate) fn process_asset(
        &self,
        asset_id: i64,
        source: &Path,
        format: ImageFormat,
    ) -> Result<()> {
        let limits = picker_imaging::DecodeLimits::default();
        let cache = picker_imaging::ThumbnailCache::new(&self.cache_root);
        let decoder = PlatformDecoder;
        cache.generate(&decoder, asset_id, source, format, &limits)?;
        let decoded = decoder.decode_file(source, format, &limits)?;
        self.store
            .assets()
            .set_perceptual_hash(asset_id, &dhash(&decoded))?;
        let extracted = extract_colors(&decoded);
        let colors = extracted
            .into_iter()
            .map(|color| (color.hex, color.ratio))
            .collect::<Vec<_>>();
        self.store
            .colors()
            .set_auto(asset_id, &colors, "lab-kmeans-v1")?;
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

    pub fn asset_summary(&self, asset_id: i64) -> Result<AssetSummary> {
        self.summary(self.store.assets().get(asset_id)?)
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
            colors: self.store.colors().list(asset_id)?,
        })
    }

    pub fn set_asset_colors(
        &self,
        asset_id: i64,
        colors: &[(String, f64)],
    ) -> Result<Vec<AssetColor>> {
        Ok(self.store.colors().set_user(asset_id, colors)?)
    }

    /// Removes the manual override and recomputes colors from a present location.
    pub fn reset_asset_colors(&self, asset_id: i64) -> Result<Vec<AssetColor>> {
        self.store.colors().clear_manual(asset_id)?;
        let locations = self.store.locations().list_for_asset(asset_id)?;
        for location in locations {
            if location.status != LocationStatus::Present {
                continue;
            }
            let root = self.store.roots().get(location.root_id)?;
            let source = Path::new(&root.absolute_path).join(&location.relative_path);
            let Some(format) = ImageFormat::from_path(&source) else {
                continue;
            };
            let limits = picker_imaging::DecodeLimits::default();
            if let Ok(decoded) = RustFallbackDecoder.decode_file(&source, format, &limits) {
                let colors = extract_colors(&decoded)
                    .into_iter()
                    .map(|color| (color.hex, color.ratio))
                    .collect::<Vec<_>>();
                self.store
                    .colors()
                    .set_auto(asset_id, &colors, "lab-kmeans-v1")?;
                break;
            }
        }
        Ok(self.store.colors().list(asset_id)?)
    }

    /// Installs the active embedder and rebuilds the in-memory vector index.
    pub fn set_embedder(&self, embedder: Option<Arc<dyn Embedder>>) -> Result<()> {
        if let Some(embedder) = &embedder {
            let model_id = embedder.model_id().to_owned();
            let model_version = embedder.model_version().to_owned();
            // Read before loading, so vectors written meanwhile are synced later.
            let loaded_at = unix_now();
            let stored = self
                .store
                .embeddings()
                .list_for_model(&model_id, &model_version)?;
            let mut index = BruteForceIndex::new(embedder.dimensions());
            for embedding in stored {
                if embedding.dimensions as usize != embedder.dimensions() {
                    return Err(LibraryError::InvalidInput(format!(
                        "stored embedding dimension {} does not match model dimension {}",
                        embedding.dimensions,
                        embedder.dimensions()
                    )));
                }
                index
                    .upsert(embedding.asset_id, &embedding.vector)
                    .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
            }
            *self
                .vectors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(VectorState {
                model_id,
                model_version,
                index,
                synced_at: loaded_at,
            });
            *self
                .embedder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(embedder));
        } else {
            *self
                .embedder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            *self
                .vectors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
        Ok(())
    }

    /// The embedding model in use, if any.
    #[must_use]
    pub fn current_embedder(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Sets the embedder of a worker library. Unlike [`Library::set_embedder`]
    /// it does not load the vector index, which workers never search.
    pub(crate) fn set_worker_embedder(&self, embedder: Option<Arc<dyn Embedder>>) {
        *self
            .embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = embedder;
        *self
            .tagger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Adds embeddings written by the workers to the in-memory index.
    /// Returns the number of vectors read.
    pub fn sync_vectors(&self) -> Result<usize> {
        let mut vectors = self
            .vectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(state) = vectors.as_mut() else {
            return Ok(0);
        };
        let now = unix_now();
        let updated = self.store.embeddings().list_updated_since(
            &state.model_id,
            &state.model_version,
            state.synced_at,
        )?;
        for embedding in &updated {
            state
                .index
                .upsert(embedding.asset_id, &embedding.vector)
                .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        }
        state.synced_at = now;
        Ok(updated.len())
    }

    /// Empties the in-memory vector index while keeping the model.
    pub(crate) fn clear_vector_index(&self) {
        if let Some(state) = self
            .vectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            state.index = BruteForceIndex::new(state.index.dimensions());
        }
    }

    pub fn embedding_progress(&self) -> Result<Option<EmbeddingProgress>> {
        let vectors = self
            .vectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(state) = vectors.as_ref() else {
            return Ok(None);
        };
        Ok(Some(EmbeddingProgress {
            model_id: state.model_id.clone(),
            model_version: state.model_version.clone(),
            dimensions: u32::try_from(state.index.dimensions()).unwrap_or(u32::MAX),
            indexed: u32::try_from(state.index.len()).unwrap_or(u32::MAX),
            missing: self
                .store
                .embeddings()
                .count_missing(&state.model_id, &state.model_version)?,
        }))
    }

    #[must_use]
    pub fn embedder_dimensions(&self) -> Option<usize> {
        self.embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|embedder| embedder.dimensions())
    }

    /// Embeds up to `limit` assets that lack the active model's vector.
    pub fn index_embeddings(&self, limit: u32) -> Result<usize> {
        let embedder = self
            .embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        let model_id = embedder.model_id().to_owned();
        let model_version = embedder.model_version().to_owned();
        let missing =
            self.store
                .embeddings()
                .missing_asset_ids(&model_id, &model_version, limit)?;
        let mut processed = 0;
        for asset_id in missing {
            let result = self.embed_one(&embedder, asset_id);
            match result {
                Ok(vector) => {
                    self.store
                        .embeddings()
                        .upsert(asset_id, &model_id, &model_version, &vector)?;
                    if let Some(vectors) = self
                        .vectors
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_mut()
                    {
                        vectors
                            .index
                            .upsert(asset_id, &vector)
                            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
                    }
                    self.store.stage_states().upsert(
                        StageTarget::Asset(asset_id),
                        Stage::Embedding,
                        StageStatus::Succeeded,
                        &format!("{model_id}@{model_version}"),
                        None,
                    )?;
                    processed += 1;
                }
                Err(error) => {
                    let _ = self.store.stage_states().upsert(
                        StageTarget::Asset(asset_id),
                        Stage::Embedding,
                        StageStatus::Failed,
                        &format!("{model_id}@{model_version}"),
                        Some(&error.to_string()),
                    );
                }
            }
        }
        Ok(processed)
    }

    fn embed_one(&self, embedder: &Arc<dyn Embedder>, asset_id: i64) -> Result<Vec<f32>> {
        let source = self.present_source(asset_id)?.ok_or_else(|| {
            LibraryError::InvalidInput("利用可能な画像ファイルがありません。".to_owned())
        })?;
        embedder
            .embed_image(&source)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))
    }

    pub(crate) fn present_source(&self, asset_id: i64) -> Result<Option<PathBuf>> {
        for location in self.store.locations().list_for_asset(asset_id)? {
            if location.status != LocationStatus::Present {
                continue;
            }
            let root = self.store.roots().get(location.root_id)?;
            return Ok(Some(
                Path::new(&root.absolute_path).join(&location.relative_path),
            ));
        }
        Ok(None)
    }

    /// Embeds text and returns nearest assets with cosine scores.
    /// Adds `zero_shot` suggestions for assets that still need auto-tagging.
    pub fn auto_tag(&self, limit: u32) -> Result<usize> {
        let state = self.ensure_tagger()?;
        let Some(state) = state else {
            return Err(LibraryError::InvalidInput(
                "埋め込みモデルが未設定です。".to_owned(),
            ));
        };
        let asset_ids = self.store.embeddings().missing_auto_tag(
            &state.model_id,
            &state.model_version,
            AUTO_TAG_STAGE_VERSION,
            limit.clamp(1, 500),
        )?;
        let mut suggested = 0;
        for asset_id in asset_ids {
            suggested += self.auto_tag_with(&state, asset_id)?;
        }
        Ok(suggested)
    }

    /// Adds `zero_shot` suggestions for one asset. Returns the number of
    /// suggestions, or zero when the asset has no embedding yet.
    pub(crate) fn auto_tag_asset(&self, asset_id: i64) -> Result<usize> {
        let state = self
            .ensure_tagger()?
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        self.auto_tag_with(&state, asset_id)
    }

    fn auto_tag_with(&self, state: &TaggerState, asset_id: i64) -> Result<usize> {
        let embedding =
            self.store
                .embeddings()
                .get(asset_id, &state.model_id, &state.model_version)?;
        let Some(embedding) = embedding else {
            return Ok(0);
        };
        match state.tagger.suggest(&embedding.vector) {
            Ok(suggestions) => {
                let mapped: Vec<(i64, f64)> = suggestions
                    .into_iter()
                    .filter_map(|suggestion| {
                        state
                            .tag_ids
                            .get(&suggestion.tag_id)
                            .map(|tag_id| (*tag_id, suggestion.confidence))
                    })
                    .collect();
                self.store.tag_assignments().replace_suggestions(
                    asset_id,
                    picker_store::AssignmentSource::ZeroShot,
                    &state.model_id,
                    &state.model_version,
                    &mapped,
                )?;
                self.store.stage_states().upsert(
                    StageTarget::Asset(asset_id),
                    Stage::AutoTag,
                    StageStatus::Succeeded,
                    AUTO_TAG_STAGE_VERSION,
                    None,
                )?;
                Ok(mapped.len())
            }
            Err(error) => {
                self.store.stage_states().upsert(
                    StageTarget::Asset(asset_id),
                    Stage::AutoTag,
                    StageStatus::Failed,
                    AUTO_TAG_STAGE_VERSION,
                    Some(&error.to_string()),
                )?;
                Ok(0)
            }
        }
    }

    fn ensure_tagger(&self) -> Result<Option<Arc<TaggerState>>> {
        let embedder = self
            .embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        let mut cached = self
            .tagger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = cached.as_ref() {
            if state.model_id == embedder.model_id()
                && state.model_version == embedder.model_version()
            {
                return Ok(Some(Arc::clone(state)));
            }
        }
        let vocabulary = ZeroShotTagger::vocabulary()
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        let mut tag_ids = std::collections::HashMap::with_capacity(vocabulary.len());
        for entry in &vocabulary {
            let tag = match self.store.tags().find("general", &entry.id)? {
                Some(tag) => tag,
                None => self.store.tags().create(&NewTag {
                    namespace: "general",
                    canonical_name: &entry.id,
                    display_name_ja: &entry.display_name_ja,
                    display_name_en: Some(&entry.display_name_en),
                    parent_id: None,
                    color: None,
                    origin: TagOrigin::ZeroShot,
                })?,
            };
            tag_ids.insert(entry.id.clone(), tag.id);
        }
        let tagger = ZeroShotTagger::new(embedder.as_ref(), vocabulary)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        let state = Arc::new(TaggerState {
            model_id: embedder.model_id().to_owned(),
            model_version: embedder.model_version().to_owned(),
            tagger,
            tag_ids,
        });
        *cached = Some(Arc::clone(&state));
        Ok(Some(state))
    }

    /// Searches tag names first, then ranks the remaining assets semantically.
    pub fn search_text(&self, query: &str, limit: u32) -> Result<Vec<(i64, f32)>> {
        let limit = limit.clamp(1, 500) as usize;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(limit);
        let mut seen = std::collections::HashSet::new();
        for asset_id in self
            .store
            .tags()
            .asset_ids_matching(query)?
            .into_iter()
            .take(limit)
        {
            if seen.insert(asset_id) {
                results.push((asset_id, 1.0));
            }
        }
        if results.len() == limit {
            return Ok(results);
        }
        for (asset_id, score) in self.semantic_search(query, limit as u32)? {
            if results.len() == limit {
                break;
            }
            if seen.insert(asset_id) {
                results.push((asset_id, score));
            }
        }
        Ok(results)
    }

    pub fn semantic_search(&self, query: &str, limit: u32) -> Result<Vec<(i64, f32)>> {
        let embedder = self
            .embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        let vector = embedder
            .embed_text(query)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        self.search_vector(&vector, limit)
    }

    /// Finds assets whose stored embedding is closest to `asset_id`.
    pub fn similar_assets(&self, asset_id: i64, limit: u32) -> Result<Vec<(i64, f32)>> {
        let vectors = self
            .vectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = vectors
            .as_ref()
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        let embedding = self
            .store
            .embeddings()
            .get(asset_id, &state.model_id, &state.model_version)?
            .ok_or_else(|| LibraryError::InvalidInput("この画像は未処理です。".to_owned()))?;
        state
            .index
            .search(&embedding.vector, limit.clamp(1, 500) as usize + 1)
            .map(|results| {
                results
                    .into_iter()
                    .filter(|(id, _)| *id != asset_id)
                    .take(limit as usize)
                    .collect()
            })
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))
    }

    fn search_vector(&self, vector: &[f32], limit: u32) -> Result<Vec<(i64, f32)>> {
        let vectors = self
            .vectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = vectors
            .as_ref()
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        state
            .index
            .search(vector, limit.clamp(1, 500) as usize)
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))
    }

    pub fn list_albums(&self) -> Result<Vec<picker_store::AlbumWithCount>> {
        Ok(self.store.albums().list()?)
    }

    pub fn create_album(&self, name: &str) -> Result<picker_store::Album> {
        Ok(self.store.albums().create(name, 0)?)
    }

    pub fn rename_album(&self, album_id: i64, name: &str) -> Result<picker_store::Album> {
        Ok(self.store.albums().rename(album_id, name)?)
    }

    pub fn delete_album(&self, album_id: i64) -> Result<bool> {
        Ok(self.store.albums().delete(album_id)?)
    }

    pub fn add_assets_to_album(&self, album_id: i64, asset_ids: &[i64]) -> Result<usize> {
        Ok(self.store.albums().add_assets(album_id, asset_ids)?)
    }

    pub fn remove_assets_from_album(&self, album_id: i64, asset_ids: &[i64]) -> Result<usize> {
        Ok(self.store.albums().remove_assets(album_id, asset_ids)?)
    }

    pub fn reorder_album(&self, album_id: i64, asset_ids: &[i64]) -> Result<()> {
        Ok(self.store.albums().reorder(album_id, asset_ids)?)
    }

    pub fn album_assets(&self, album_id: i64) -> Result<Vec<AssetSummary>> {
        let items = self.store.albums().list_items(album_id)?;
        let mut assets = Vec::with_capacity(items.len());
        for item in items {
            assets.push(self.summary(self.store.assets().get(item.asset_id)?)?);
        }
        Ok(assets)
    }

    pub fn album_asset_ids(&self, album_id: i64) -> Result<Vec<i64>> {
        Ok(self
            .store
            .albums()
            .list_items(album_id)?
            .into_iter()
            .map(|item| item.asset_id)
            .collect())
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
        if state == TagState::Suggested {
            return Err(LibraryError::InvalidInput(
                "user decisions must be confirmed or rejected".to_owned(),
            ));
        }
        let mut previous = Vec::with_capacity(asset_ids.len());
        for asset_id in asset_ids {
            previous.push((
                *asset_id,
                self.store
                    .tag_assignments()
                    .get_optional(*asset_id, tag_id)?,
            ));
        }
        let changed = self
            .store
            .tag_assignments()
            .set_user_many(asset_ids, tag_id, state)?;
        for (asset_id, assignment) in previous {
            if assignment
                .as_ref()
                .is_some_and(|assignment| assignment.state == state)
            {
                continue;
            }
            let action = match state {
                TagState::Confirmed => match assignment.as_ref() {
                    Some(assignment) if assignment.state == TagState::Suggested => {
                        FeedbackAction::Approved
                    }
                    _ => FeedbackAction::ManualAdd,
                },
                TagState::Rejected => FeedbackAction::Rejected,
                TagState::Suggested => unreachable!(),
            };
            self.store.feedback().record(
                asset_id,
                tag_id,
                action,
                assignment.as_ref().and_then(|value| value.confidence),
                assignment.as_ref(),
            )?;
        }
        Ok(changed)
    }

    pub fn remove_tag_from_assets(&self, asset_ids: &[i64], tag_id: i64) -> Result<usize> {
        let mut previous = Vec::with_capacity(asset_ids.len());
        for asset_id in asset_ids {
            if let Some(assignment) = self
                .store
                .tag_assignments()
                .get_optional(*asset_id, tag_id)?
            {
                if assignment.source == picker_store::AssignmentSource::User {
                    previous.push((*asset_id, assignment));
                }
            }
        }
        let changed = self
            .store
            .tag_assignments()
            .remove_user_many(asset_ids, tag_id)?;
        for (asset_id, assignment) in previous {
            self.store.feedback().record(
                asset_id,
                tag_id,
                FeedbackAction::ManualRemove,
                assignment.confidence,
                Some(&assignment),
            )?;
        }
        Ok(changed)
    }

    pub fn convert_tag_to_custom(&self, tag_id: i64) -> Result<Tag> {
        Ok(self.store.tags().convert_to_custom(tag_id)?)
    }

    /// Recomputes prototype (or tag-name) suggestions for one custom tag.
    pub fn suggest_custom_tag(&self, tag_id: i64, limit: u32) -> Result<usize> {
        const STAGE_VERSION: &str = "prototype-v1";
        let embedder = self
            .embedder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned()))?;
        let (model_id, model_version) = {
            let vectors = self
                .vectors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let state = vectors.as_ref().ok_or_else(|| {
                LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned())
            })?;
            (state.model_id.clone(), state.model_version.clone())
        };
        let tag = self.store.tags().get(tag_id)?;
        if tag.namespace != "custom" {
            return Err(LibraryError::InvalidInput(
                "custom suggestions require a custom tag".to_owned(),
            ));
        }
        let positives = self
            .store
            .tag_assignments()
            .list_for_tag(tag_id, TagState::Confirmed)?;
        let rejected = self
            .store
            .tag_assignments()
            .list_for_tag(tag_id, TagState::Rejected)?;
        if self.store.classifiers().get(tag_id)?.is_some()
            || (positives.len() >= 10 && rejected.len() >= 10)
        {
            return self.train_custom_tag(tag_id);
        }
        let mut prototype = Vec::new();
        for assignment in &positives {
            if let Some(embedding) =
                self.store
                    .embeddings()
                    .get(assignment.asset_id, &model_id, &model_version)?
            {
                prototype.push(embedding.vector);
            }
        }
        let query = if prototype.len() >= 3 {
            let mut average = vec![0.0_f32; embedder.dimensions()];
            for vector in &prototype {
                if vector.len() != average.len() {
                    return Err(LibraryError::InvalidInput(
                        "embedding dimensions do not match the model".to_owned(),
                    ));
                }
                for (sum, value) in average.iter_mut().zip(vector) {
                    *sum += value;
                }
            }
            let norm = average
                .iter()
                .map(|value| f64::from(*value) * f64::from(*value))
                .sum::<f64>()
                .sqrt();
            if norm > 0.0 {
                for value in &mut average {
                    *value = (f64::from(*value) / norm) as f32;
                }
            }
            average
        } else {
            embedder
                .embed_text(&format!("{} {}", tag.display_name_ja, tag.canonical_name))
                .map_err(|error| LibraryError::InvalidInput(error.to_string()))?
        };
        let results = {
            let vectors = self
                .vectors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            vectors
                .as_ref()
                .expect("checked above")
                .index
                .search(&query, 500)
                .map_err(|error| LibraryError::InvalidInput(error.to_string()))?
        };
        let threshold = if prototype.len() >= 3 {
            0.35_f32
        } else {
            0.30_f32
        };
        let cap = limit.clamp(1, 500) as usize;
        let mut suggested = 0;
        for (asset_id, score) in results {
            self.store.tag_assignments().clear_suggested(
                asset_id,
                tag_id,
                picker_store::AssignmentSource::CustomClassifier,
            )?;
            if score >= threshold && suggested < cap {
                self.store.tag_assignments().upsert_suggested(
                    asset_id,
                    tag_id,
                    picker_store::AssignmentSource::CustomClassifier,
                    f64::from(score),
                    &model_id,
                    &model_version,
                )?;
                suggested += 1;
            }
            self.store.stage_states().upsert(
                StageTarget::Asset(asset_id),
                Stage::CustomSuggest,
                StageStatus::Succeeded,
                STAGE_VERSION,
                None,
            )?;
        }
        Ok(suggested)
    }

    /// Trains the stage-B logistic classifier and refreshes its suggestions.
    pub fn train_custom_tag(&self, tag_id: i64) -> Result<usize> {
        const STAGE_VERSION: &str = "logistic-v1";
        let tag = self.store.tags().get(tag_id)?;
        if tag.namespace != "custom" {
            return Err(LibraryError::InvalidInput(
                "custom classifiers require a custom tag".to_owned(),
            ));
        }
        let (model_id, model_version, dimensions) = {
            let vectors = self
                .vectors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let state = vectors.as_ref().ok_or_else(|| {
                LibraryError::InvalidInput("埋め込みモデルが未設定です。".to_owned())
            })?;
            (
                state.model_id.clone(),
                state.model_version.clone(),
                state.index.dimensions(),
            )
        };
        let embeddings = self
            .store
            .embeddings()
            .list_for_model(&model_id, &model_version)?;
        let positives: std::collections::HashSet<i64> = self
            .store
            .tag_assignments()
            .list_for_tag(tag_id, TagState::Confirmed)?
            .into_iter()
            .map(|assignment| assignment.asset_id)
            .collect();
        let mut negatives: std::collections::BTreeSet<i64> = self
            .store
            .tag_assignments()
            .list_for_tag(tag_id, TagState::Rejected)?
            .into_iter()
            .map(|assignment| assignment.asset_id)
            .collect();
        negatives.extend(
            self.store
                .tag_assignments()
                .confirmed_asset_ids_excluding(tag_id, 5000)?,
        );
        negatives.retain(|asset_id| !positives.contains(asset_id));
        if positives.len() < 10 || negatives.len() < 10 {
            return Err(LibraryError::InvalidInput(
                "10 positives and 10 negatives are required".to_owned(),
            ));
        }
        let mut samples = Vec::with_capacity(positives.len() + negatives.len());
        for embedding in &embeddings {
            if positives.contains(&embedding.asset_id) {
                samples.push((embedding.vector.clone(), true));
            } else if negatives.contains(&embedding.asset_id) {
                samples.push((embedding.vector.clone(), false));
            }
        }
        let mut classifier = LogisticRegression::new(dimensions);
        classifier.fit(&samples, 0.001, 5.0, 20);
        self.store.classifiers().upsert(
            tag_id,
            u32::try_from(dimensions).unwrap_or(u32::MAX),
            classifier.weights(),
            classifier.bias(),
        )?;
        let mut suggested = 0;
        for embedding in embeddings {
            let probability = classifier.predict(&embedding.vector);
            self.store.tag_assignments().clear_suggested(
                embedding.asset_id,
                tag_id,
                picker_store::AssignmentSource::CustomClassifier,
            )?;
            if probability >= 0.5 {
                self.store.tag_assignments().upsert_suggested(
                    embedding.asset_id,
                    tag_id,
                    picker_store::AssignmentSource::CustomClassifier,
                    probability,
                    &model_id,
                    &model_version,
                )?;
                suggested += 1;
            }
            self.store.stage_states().upsert(
                StageTarget::Asset(embedding.asset_id),
                Stage::CustomSuggest,
                StageStatus::Succeeded,
                STAGE_VERSION,
                None,
            )?;
        }
        Ok(suggested)
    }

    /// Returns review candidates for a tag, mixing confident and borderline items.
    pub fn review_candidates(&self, tag_id: i64, total: u32, high_ratio: f64) -> Result<Vec<i64>> {
        let assignments = self
            .store
            .tag_assignments()
            .list_for_tag(tag_id, TagState::Suggested)?;
        let scored: Vec<(i64, f64)> = assignments
            .into_iter()
            .map(|assignment| (assignment.asset_id, assignment.confidence.unwrap_or(0.0)))
            .collect();
        Ok(
            select_review_candidates(&scored, total.clamp(1, 100) as usize, high_ratio)
                .into_iter()
                .map(|(asset_id, _)| asset_id)
                .collect(),
        )
    }

    /// Applies a review batch and returns the recorded feedback event ids.
    pub fn confirm_review(&self, tag_id: i64, keep: &[i64], reject: &[i64]) -> Result<Vec<i64>> {
        let tag = self.store.tags().get(tag_id)?;
        let mut events = Vec::new();
        for (asset_ids, state) in [(keep, TagState::Confirmed), (reject, TagState::Rejected)] {
            if asset_ids.is_empty() {
                continue;
            }
            self.set_tag_on_assets(asset_ids, tag_id, state)?;
            for asset_id in asset_ids {
                if let Some(event) = self
                    .store
                    .feedback()
                    .list_for_asset(*asset_id)?
                    .into_iter()
                    .rev()
                    .find(|event| event.tag_id == Some(tag_id))
                {
                    events.push(event.id);
                }
            }
        }
        if tag.namespace == "custom" {
            let _ = self.train_custom_tag(tag_id);
        }
        Ok(events)
    }

    /// Restores the assignment that a feedback event replaced.
    pub fn undo_feedback(&self, event_id: i64) -> Result<FeedbackEvent> {
        let event = self
            .store
            .feedback()
            .get(event_id)?
            .ok_or_else(|| LibraryError::InvalidInput("feedback event not found".to_owned()))?;
        let tag_id = event
            .tag_id
            .ok_or_else(|| LibraryError::InvalidInput("tag was deleted".to_owned()))?;
        let current = self
            .store
            .tag_assignments()
            .get_optional(event.asset_id, tag_id)?;
        self.store
            .tag_assignments()
            .restore(event.asset_id, tag_id, &event)?;
        Ok(self.store.feedback().record(
            event.asset_id,
            tag_id,
            FeedbackAction::Undo,
            current.as_ref().and_then(|value| value.confidence),
            current.as_ref(),
        )?)
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

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}
