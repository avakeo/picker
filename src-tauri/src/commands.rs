use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use picker_core::{Library, ThumbnailState};
use picker_store::{AssetFilter, LocationStatus, RootStatus, TagFilter, TagMatchMode, TagState};
use tauri::State;

use picker_ml::ModelRegistry;
use std::sync::Arc;

use crate::contract::{
    AlbumAssetsRequest, AlbumCreateRequest, AlbumRenameRequest, AlbumReorderRequest, AlbumView,
    AssetDetailView, AssetFilterRequest, AssetMetadataView, AssetPageView, AssetSearchRequest,
    AssetSummaryView, AssetTagView, ColorSetRequest, ColorView, CustomSuggestRequest,
    EmbeddingIndexRequest, EmbeddingStatusView, EventContract, FinderRevealRequest, IdRequest,
    IndexProgressChanged, IndexProgressChangedPayload, IndexStageProgress, IndexStatusView,
    LocationView, ModelIdRequest, ModelProgressEvent, ModelView, RootAddRequest,
    RootEnabledRequest, RootView, SemanticSearchRequest, SimilarAssetsRequest, StageProgressView,
    TagAliasDeleteRequest, TagAliasRequest, TagAliasView, TagAssignRequest, TagCreateRequest,
    TagIdRequest, TagMergeRequest, TagParentRequest, TagRemoveRequest, TagRenameRequest, TagView,
    ViewportRequest,
};

/// Shared application state managed by Tauri.
pub struct AppState {
    library: Mutex<Library>,
    models: Arc<ModelRegistry>,
}

impl AppState {
    #[must_use]
    pub fn new(library: Library, models: ModelRegistry) -> Self {
        Self {
            library: Mutex::new(library),
            models: Arc::new(models),
        }
    }

    fn library(&self) -> std::sync::MutexGuard<'_, Library> {
        self.library
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

type CommandResult<T> = Result<T, String>;

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub(crate) fn parse_id(value: &str) -> CommandResult<i64> {
    let id = value.parse::<i64>().map_err(|_| "invalid id".to_owned())?;
    if id <= 0 {
        return Err("invalid id".to_owned());
    }
    Ok(id)
}

fn root_view(summary: picker_core::RootSummary) -> RootView {
    RootView {
        id: summary.root.id.to_string(),
        path: summary.root.absolute_path.clone(),
        display_name: summary.root.display_name.clone(),
        enabled: summary.root.enabled,
        online: summary.root.status == RootStatus::Online,
        image_count: summary.image_count,
    }
}

fn summary_view(summary: picker_core::AssetSummary) -> AssetSummaryView {
    AssetSummaryView {
        id: summary.id.to_string(),
        width: summary.width,
        height: summary.height,
        format: summary.format,
        captured_at: summary.captured_at,
        thumbnail_state: match summary.thumbnail {
            ThumbnailState::Missing => "missing",
            ThumbnailState::Ready => "ready",
            ThumbnailState::Failed => "failed",
        }
        .to_owned(),
        location_status: match summary.location_status {
            LocationStatus::Present => "present",
            LocationStatus::Offline => "offline",
            LocationStatus::Missing => "missing",
        }
        .to_owned(),
    }
}

fn page_view(page: picker_core::AssetPage) -> AssetPageView {
    AssetPageView {
        items: page.items.into_iter().map(summary_view).collect(),
        next_cursor: page.next_cursor.map(|id| id.to_string()),
        total: page.total,
        total_is_exact: page.total_is_exact,
    }
}

fn detail_view(detail: picker_core::AssetDetail) -> AssetDetailView {
    AssetDetailView {
        summary: summary_view(detail.summary),
        file_size: detail.file_size,
        locations: detail
            .locations
            .into_iter()
            .map(|location| LocationView {
                id: location.id.to_string(),
                root_id: location.root_id.to_string(),
                relative_path: location.relative_path,
                absolute_path: location.absolute_path.to_string_lossy().into_owned(),
                status: match location.status {
                    LocationStatus::Present => "present",
                    LocationStatus::Offline => "offline",
                    LocationStatus::Missing => "missing",
                }
                .to_owned(),
            })
            .collect(),
        metadata: detail.metadata.map(|metadata| AssetMetadataView {
            camera_make: metadata.camera_make,
            camera_model: metadata.camera_model,
            lens: metadata.lens,
            exposure_time: metadata.exposure_time,
            f_number: metadata.f_number,
            iso: metadata.iso,
            focal_length_mm: metadata.focal_length_mm,
            gps_latitude: metadata.gps_latitude,
            gps_longitude: metadata.gps_longitude,
        }),
        tags: detail
            .tags
            .into_iter()
            .map(|assigned| AssetTagView {
                tag_id: assigned.tag.id.to_string(),
                canonical_name: assigned.tag.canonical_name,
                display_name_ja: assigned.tag.display_name_ja,
                state: assigned.assignment.state.as_str().to_owned(),
                source: assigned.assignment.source.as_str().to_owned(),
                confidence: assigned.assignment.confidence,
            })
            .collect(),
        colors: detail
            .colors
            .into_iter()
            .map(|color| ColorView {
                rank: color.rank,
                hex: color.hex,
                ratio: color.ratio,
                source: color.source.as_str().to_owned(),
            })
            .collect(),
    }
}

fn tag_view(tag: picker_store::TagWithCount) -> TagView {
    TagView {
        id: tag.tag.id.to_string(),
        namespace: tag.tag.namespace,
        canonical_name: tag.tag.canonical_name,
        display_name_ja: tag.tag.display_name_ja,
        display_name_en: tag.tag.display_name_en,
        parent_id: tag.tag.parent_id.map(|id| id.to_string()),
        color: tag.tag.color,
        origin: tag.tag.origin.as_str().to_owned(),
        asset_count: tag.asset_count,
    }
}

fn filter_from_request(request: &AssetFilterRequest) -> CommandResult<AssetFilter> {
    let mode = match request.tag_mode.as_str() {
        "all" => TagMatchMode::All,
        "any" => TagMatchMode::Any,
        "none" => TagMatchMode::None,
        other => return Err(format!("invalid tag mode: {other}")),
    };
    let tag_ids = request
        .tag_ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<CommandResult<Vec<_>>>()?;
    Ok(AssetFilter {
        tags: TagFilter {
            tag_ids,
            mode,
            include_suggested: request.include_suggested,
        },
        formats: request.formats.clone(),
        min_width: request.min_width,
        min_height: request.min_height,
        captured_from: request.captured_from,
        captured_to: request.captured_to,
    })
}

fn status_view(status: picker_core::IndexStatus) -> IndexStatusView {
    IndexStatusView {
        stages: status
            .stages
            .into_iter()
            .map(|stage| StageProgressView {
                stage: stage.stage.as_str().to_owned(),
                completed: stage.completed,
                pending: stage.pending,
                failed: stage.failed,
            })
            .collect(),
        paused: status.paused,
        total_assets: status.total_assets,
    }
}

fn to_event(status: &IndexStatusView) -> IndexProgressChangedPayload {
    IndexProgressChangedPayload {
        stages: status
            .stages
            .iter()
            .map(|stage| IndexStageProgress {
                stage: stage.stage.clone(),
                completed: stage.completed,
                pending: stage.pending,
                failed: stage.failed,
            })
            .collect(),
        paused: status.paused,
    }
}

#[tauri::command]
pub fn roots_list(state: State<'_, AppState>) -> CommandResult<Vec<RootView>> {
    state
        .library()
        .list_roots()
        .map(|roots| roots.into_iter().map(root_view).collect())
        .map_err(error)
}

#[tauri::command]
pub fn root_add(state: State<'_, AppState>, request: RootAddRequest) -> CommandResult<RootView> {
    let library = state.library();
    let root = library
        .add_root(&request.path, &request.display_name)
        .map_err(error)?;
    Ok(root_view(picker_core::RootSummary {
        root,
        image_count: 0,
    }))
}

#[tauri::command]
pub fn root_remove(state: State<'_, AppState>, request: IdRequest) -> CommandResult<bool> {
    let id = parse_id(&request.id)?;
    state.library().remove_root(id).map_err(error)
}

#[tauri::command]
pub fn root_set_enabled(
    state: State<'_, AppState>,
    request: RootEnabledRequest,
) -> CommandResult<RootView> {
    let id = parse_id(&request.id)?;
    let root = state
        .library()
        .set_root_enabled(id, request.enabled)
        .map_err(error)?;
    Ok(root_view(picker_core::RootSummary {
        root,
        image_count: 0,
    }))
}

#[tauri::command]
pub fn rescan_root(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: IdRequest,
) -> CommandResult<IndexStatusView> {
    use tauri::Emitter;

    let id = parse_id(&request.id)?;
    let library = state.library();
    library.scan_root(id).map_err(error)?;
    let status = status_view(library.index_status().map_err(error)?);
    let _ = app.emit(IndexProgressChanged::NAME, to_event(&status));
    Ok(status)
}

#[tauri::command]
pub fn asset_search(
    state: State<'_, AppState>,
    request: AssetSearchRequest,
) -> CommandResult<AssetPageView> {
    let after = match request.cursor.as_deref() {
        Some(cursor) => parse_id(cursor)?,
        None => 0,
    };
    state
        .library()
        .asset_page(after, request.page_size)
        .map(page_view)
        .map_err(error)
}

#[tauri::command]
pub fn asset_detail(
    state: State<'_, AppState>,
    request: IdRequest,
) -> CommandResult<AssetDetailView> {
    let id = parse_id(&request.id)?;
    state
        .library()
        .asset_detail(id)
        .map(detail_view)
        .map_err(error)
}

#[tauri::command]
pub fn viewport_update(
    state: State<'_, AppState>,
    request: ViewportRequest,
) -> CommandResult<usize> {
    let ids = request
        .asset_ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<CommandResult<Vec<_>>>()?;
    state.library().prioritize_viewport(&ids).map_err(error)
}

#[tauri::command]
pub fn index_status(state: State<'_, AppState>) -> CommandResult<IndexStatusView> {
    state
        .library()
        .index_status()
        .map(status_view)
        .map_err(error)
}

#[tauri::command]
pub fn pause_index(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<IndexStatusView> {
    use tauri::Emitter;

    let library = state.library();
    library.pause_index().map_err(error)?;
    let status = status_view(library.index_status().map_err(error)?);
    let _ = app.emit(IndexProgressChanged::NAME, to_event(&status));
    Ok(status)
}

#[tauri::command]
pub fn resume_index(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<IndexStatusView> {
    use tauri::Emitter;

    let library = state.library();
    library.resume_index().map_err(error)?;
    let status = status_view(library.index_status().map_err(error)?);
    let _ = app.emit(IndexProgressChanged::NAME, to_event(&status));
    Ok(status)
}

#[tauri::command]
pub fn finder_reveal(
    state: State<'_, AppState>,
    request: FinderRevealRequest,
) -> CommandResult<String> {
    let asset_id = parse_id(&request.asset_id)?;
    let location_id = parse_id(&request.location_id)?;
    let detail = state.library().asset_detail(asset_id).map_err(error)?;
    let location = detail
        .locations
        .into_iter()
        .find(|location| location.id == location_id)
        .ok_or_else(|| "location not found".to_owned())?;
    reveal(&location.absolute_path)?;
    Ok(location.absolute_path.to_string_lossy().into_owned())
}

fn reveal(path: &Path) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or_else(|| Path::new(".")));
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to reveal file: {error}"))
}

pub(crate) fn default_database(data_dir: &Path) -> PathBuf {
    data_dir.join("library.sqlite3")
}

pub(crate) fn default_cache(data_dir: &Path) -> PathBuf {
    data_dir.join("thumbnails")
}

#[tauri::command]
pub fn tags_list(state: State<'_, AppState>) -> CommandResult<Vec<TagView>> {
    state
        .library()
        .list_tags()
        .map(|tags| tags.into_iter().map(tag_view).collect())
        .map_err(error)
}

#[tauri::command]
pub fn tag_create(state: State<'_, AppState>, request: TagCreateRequest) -> CommandResult<TagView> {
    let parent_id = request.parent_id.as_deref().map(parse_id).transpose()?;
    let tag = state
        .library()
        .create_tag(
            &request.canonical_name,
            &request.display_name_ja,
            parent_id,
            request.color.as_deref(),
        )
        .map_err(error)?;
    Ok(tag_view(picker_store::TagWithCount {
        tag,
        asset_count: 0,
    }))
}

#[tauri::command]
pub fn tag_rename(state: State<'_, AppState>, request: TagRenameRequest) -> CommandResult<TagView> {
    let id = parse_id(&request.id)?;
    let tag = state
        .library()
        .rename_tag(id, &request.canonical_name, &request.display_name_ja)
        .map_err(error)?;
    let count = state
        .library()
        .list_tags()
        .map_err(error)?
        .into_iter()
        .find(|tag| tag.tag.id == id)
        .map_or(0, |tag| tag.asset_count);
    Ok(tag_view(picker_store::TagWithCount {
        tag,
        asset_count: count,
    }))
}

#[tauri::command]
pub fn tag_delete(state: State<'_, AppState>, request: IdRequest) -> CommandResult<bool> {
    let id = parse_id(&request.id)?;
    state.library().delete_tag(id).map_err(error)
}

#[tauri::command]
pub fn tag_set_parent(
    state: State<'_, AppState>,
    request: TagParentRequest,
) -> CommandResult<TagView> {
    let id = parse_id(&request.id)?;
    let parent_id = request.parent_id.as_deref().map(parse_id).transpose()?;
    let tag = state
        .library()
        .set_tag_parent(id, parent_id)
        .map_err(error)?;
    Ok(tag_view(picker_store::TagWithCount {
        tag,
        asset_count: 0,
    }))
}

#[tauri::command]
pub fn tag_merge(state: State<'_, AppState>, request: TagMergeRequest) -> CommandResult<TagView> {
    let source_id = parse_id(&request.source_id)?;
    let target_id = parse_id(&request.target_id)?;
    let tag = state
        .library()
        .merge_tags(source_id, target_id)
        .map_err(error)?;
    let count = state
        .library()
        .list_tags()
        .map_err(error)?
        .into_iter()
        .find(|tag| tag.tag.id == target_id)
        .map_or(0, |tag| tag.asset_count);
    Ok(tag_view(picker_store::TagWithCount {
        tag,
        asset_count: count,
    }))
}

#[tauri::command]
pub fn tag_aliases(
    state: State<'_, AppState>,
    request: IdRequest,
) -> CommandResult<Vec<TagAliasView>> {
    let tag_id = parse_id(&request.id)?;
    state
        .library()
        .tag_aliases(tag_id)
        .map(|aliases| {
            aliases
                .into_iter()
                .map(|alias| TagAliasView {
                    id: alias.id.to_string(),
                    tag_id: alias.tag_id.to_string(),
                    alias: alias.alias,
                })
                .collect()
        })
        .map_err(error)
}

#[tauri::command]
pub fn tag_add_alias(
    state: State<'_, AppState>,
    request: TagAliasRequest,
) -> CommandResult<TagAliasView> {
    let tag_id = parse_id(&request.tag_id)?;
    let alias = state
        .library()
        .add_tag_alias(tag_id, &request.alias)
        .map_err(error)?;
    Ok(TagAliasView {
        id: alias.id.to_string(),
        tag_id: alias.tag_id.to_string(),
        alias: alias.alias,
    })
}

#[tauri::command]
pub fn tag_remove_alias(
    state: State<'_, AppState>,
    request: TagAliasDeleteRequest,
) -> CommandResult<bool> {
    let id = parse_id(&request.id)?;
    state.library().remove_tag_alias(id).map_err(error)
}

#[tauri::command]
pub fn tag_assign(state: State<'_, AppState>, request: TagAssignRequest) -> CommandResult<usize> {
    let tag_id = parse_id(&request.tag_id)?;
    let ids = request
        .asset_ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<CommandResult<Vec<_>>>()?;
    state
        .library()
        .set_tag_on_assets(&ids, tag_id, TagState::Confirmed)
        .map_err(error)
}

#[tauri::command]
pub fn tag_convert_to_custom(
    state: State<'_, AppState>,
    request: TagIdRequest,
) -> CommandResult<TagView> {
    let id = parse_id(&request.id)?;
    state.library().convert_tag_to_custom(id).map_err(error)?;
    let tags = state.library().list_tags().map_err(error)?;
    tags.into_iter()
        .find(|tag| tag.tag.id == id)
        .map(tag_view)
        .ok_or_else(|| "tag not found".to_owned())
}

#[tauri::command]
pub fn custom_suggest(
    state: State<'_, AppState>,
    request: CustomSuggestRequest,
) -> CommandResult<usize> {
    let tag_id = parse_id(&request.tag_id)?;
    state
        .library()
        .suggest_custom_tag(tag_id, request.limit)
        .map_err(error)
}

#[tauri::command]
pub fn tag_reject(state: State<'_, AppState>, request: TagAssignRequest) -> CommandResult<usize> {
    let tag_id = parse_id(&request.tag_id)?;
    let ids = request
        .asset_ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<CommandResult<Vec<_>>>()?;
    state
        .library()
        .set_tag_on_assets(&ids, tag_id, TagState::Rejected)
        .map_err(error)
}

#[tauri::command]
pub fn auto_tag(
    state: State<'_, AppState>,
    request: EmbeddingIndexRequest,
) -> CommandResult<usize> {
    state
        .library()
        .auto_tag(request.limit.clamp(1, 500))
        .map_err(error)
}

#[tauri::command]
pub fn tag_remove(state: State<'_, AppState>, request: TagRemoveRequest) -> CommandResult<usize> {
    let tag_id = parse_id(&request.tag_id)?;
    let ids = request
        .asset_ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<CommandResult<Vec<_>>>()?;
    state
        .library()
        .remove_tag_from_assets(&ids, tag_id)
        .map_err(error)
}

#[tauri::command]
pub fn asset_filter(
    state: State<'_, AppState>,
    request: AssetFilterRequest,
) -> CommandResult<AssetPageView> {
    let after = match request.cursor.as_deref() {
        Some(cursor) => parse_id(cursor)?,
        None => 0,
    };
    let filter = filter_from_request(&request)?;
    state
        .library()
        .filter_page(&filter, after, request.page_size)
        .map(page_view)
        .map_err(error)
}

#[tauri::command]
pub fn duplicates_list(state: State<'_, AppState>) -> CommandResult<Vec<AssetSummaryView>> {
    state
        .library()
        .exact_duplicate_assets()
        .map(|assets| assets.into_iter().map(summary_view).collect())
        .map_err(error)
}

#[tauri::command]
pub fn color_set(
    state: State<'_, AppState>,
    request: ColorSetRequest,
) -> CommandResult<Vec<ColorView>> {
    let asset_id = parse_id(&request.asset_id)?;
    let colors = request
        .colors
        .into_iter()
        .map(|color| (color.hex, color.ratio))
        .collect::<Vec<_>>();
    state
        .library()
        .set_asset_colors(asset_id, &colors)
        .map(|colors| {
            colors
                .into_iter()
                .map(|color| ColorView {
                    rank: color.rank,
                    hex: color.hex,
                    ratio: color.ratio,
                    source: color.source.as_str().to_owned(),
                })
                .collect()
        })
        .map_err(error)
}

#[tauri::command]
pub fn color_reset(
    state: State<'_, AppState>,
    request: IdRequest,
) -> CommandResult<Vec<ColorView>> {
    let asset_id = parse_id(&request.id)?;
    state
        .library()
        .reset_asset_colors(asset_id)
        .map(|colors| {
            colors
                .into_iter()
                .map(|color| ColorView {
                    rank: color.rank,
                    hex: color.hex,
                    ratio: color.ratio,
                    source: color.source.as_str().to_owned(),
                })
                .collect()
        })
        .map_err(error)
}

fn album_view(album: picker_store::AlbumWithCount) -> AlbumView {
    AlbumView {
        id: album.album.id.to_string(),
        name: album.album.name,
        kind: album.album.kind.as_str().to_owned(),
        asset_count: album.asset_count,
    }
}

#[tauri::command]
pub fn albums_list(state: State<'_, AppState>) -> CommandResult<Vec<AlbumView>> {
    state
        .library()
        .list_albums()
        .map(|albums| albums.into_iter().map(album_view).collect())
        .map_err(error)
}

#[tauri::command]
pub fn album_create(
    state: State<'_, AppState>,
    request: AlbumCreateRequest,
) -> CommandResult<AlbumView> {
    let album = state.library().create_album(&request.name).map_err(error)?;
    Ok(album_view(picker_store::AlbumWithCount {
        album,
        asset_count: 0,
    }))
}

#[tauri::command]
pub fn album_rename(
    state: State<'_, AppState>,
    request: AlbumRenameRequest,
) -> CommandResult<AlbumView> {
    let id = parse_id(&request.id)?;
    let album = state
        .library()
        .rename_album(id, &request.name)
        .map_err(error)?;
    let count = state
        .library()
        .list_albums()
        .map_err(error)?
        .into_iter()
        .find(|album| album.album.id == id)
        .map_or(0, |album| album.asset_count);
    Ok(album_view(picker_store::AlbumWithCount {
        album,
        asset_count: count,
    }))
}

#[tauri::command]
pub fn album_delete(state: State<'_, AppState>, request: IdRequest) -> CommandResult<bool> {
    let id = parse_id(&request.id)?;
    state.library().delete_album(id).map_err(error)
}

fn parse_asset_ids(ids: &[String]) -> CommandResult<Vec<i64>> {
    ids.iter().map(|id| parse_id(id)).collect()
}

#[tauri::command]
pub fn album_add_assets(
    state: State<'_, AppState>,
    request: AlbumAssetsRequest,
) -> CommandResult<usize> {
    let album_id = parse_id(&request.album_id)?;
    let asset_ids = parse_asset_ids(&request.asset_ids)?;
    state
        .library()
        .add_assets_to_album(album_id, &asset_ids)
        .map_err(error)
}

#[tauri::command]
pub fn album_remove_assets(
    state: State<'_, AppState>,
    request: AlbumAssetsRequest,
) -> CommandResult<usize> {
    let album_id = parse_id(&request.album_id)?;
    let asset_ids = parse_asset_ids(&request.asset_ids)?;
    state
        .library()
        .remove_assets_from_album(album_id, &asset_ids)
        .map_err(error)
}

#[tauri::command]
pub fn album_reorder(
    state: State<'_, AppState>,
    request: AlbumReorderRequest,
) -> CommandResult<()> {
    let album_id = parse_id(&request.album_id)?;
    let asset_ids = parse_asset_ids(&request.asset_ids)?;
    state
        .library()
        .reorder_album(album_id, &asset_ids)
        .map_err(error)
}

#[tauri::command]
pub fn album_assets(
    state: State<'_, AppState>,
    request: IdRequest,
) -> CommandResult<Vec<AssetSummaryView>> {
    let album_id = parse_id(&request.id)?;
    state
        .library()
        .album_assets(album_id)
        .map(|assets| assets.into_iter().map(summary_view).collect())
        .map_err(error)
}

#[tauri::command]
pub fn models_list(state: State<'_, AppState>) -> CommandResult<Vec<ModelView>> {
    Ok(state
        .models
        .list()
        .into_iter()
        .map(|model| ModelView {
            id: model.id,
            version: model.version,
            license: model.license,
            license_url: model.license_url,
            size: model.size,
            state: model.state.as_str().to_owned(),
            active: model.active,
        })
        .collect())
}

#[tauri::command]
pub async fn model_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ModelIdRequest,
) -> CommandResult<()> {
    let registry = Arc::clone(&state.models);
    tauri::async_runtime::spawn_blocking(move || {
        registry
            .install(&request.id, |progress| {
                use tauri::Emitter;
                let _ = app.emit(
                    crate::contract::ModelProgressChanged::NAME,
                    ModelProgressEvent {
                        model_id: progress.model_id,
                        version: progress.version,
                        file: progress.file,
                        file_index: u32::try_from(progress.file_index).unwrap_or(u32::MAX),
                        file_count: u32::try_from(progress.file_count).unwrap_or(u32::MAX),
                        file_downloaded: progress.file_downloaded,
                        file_total: progress.file_total,
                        total_downloaded: progress.total_downloaded,
                        total_size: progress.total_size,
                    },
                );
            })
            .map(|_| ())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(error)
}

#[tauri::command]
pub fn model_activate(state: State<'_, AppState>, request: ModelIdRequest) -> CommandResult<()> {
    state.models.set_active(&request.id).map_err(error)
}

#[tauri::command]
pub fn model_remove(state: State<'_, AppState>, request: ModelIdRequest) -> CommandResult<()> {
    let (id, version) = request
        .id
        .split_once('@')
        .ok_or_else(|| "モデル ID は id@version 形式で指定してください。".to_owned())?;
    state.models.remove(id, version).map_err(error)
}

#[tauri::command]
pub fn embedding_status(state: State<'_, AppState>) -> CommandResult<Option<EmbeddingStatusView>> {
    state
        .library()
        .embedding_progress()
        .map(|progress| {
            progress.map(|progress| EmbeddingStatusView {
                model_id: progress.model_id,
                model_version: progress.model_version,
                dimensions: progress.dimensions,
                indexed: progress.indexed,
                missing: progress.missing,
            })
        })
        .map_err(error)
}

#[tauri::command]
pub fn embedding_index(
    state: State<'_, AppState>,
    request: EmbeddingIndexRequest,
) -> CommandResult<usize> {
    state
        .library()
        .index_embeddings(request.limit.clamp(1, 500))
        .map_err(error)
}

#[tauri::command]
pub fn semantic_search(
    state: State<'_, AppState>,
    request: SemanticSearchRequest,
) -> CommandResult<Vec<AssetSummaryView>> {
    let library = state.library();
    let results = library
        .search_text(&request.query, request.limit)
        .map_err(error)?;
    let mut items = Vec::with_capacity(results.len());
    for (asset_id, _) in results {
        let summary = library.asset_summary(asset_id).map_err(error)?;
        items.push(summary_view(summary));
    }
    Ok(items)
}

#[tauri::command]
pub fn similar_assets(
    state: State<'_, AppState>,
    request: SimilarAssetsRequest,
) -> CommandResult<Vec<AssetSummaryView>> {
    let asset_id = parse_id(&request.id)?;
    let library = state.library();
    let results = library
        .similar_assets(asset_id, request.limit)
        .map_err(error)?;
    let mut items = Vec::with_capacity(results.len());
    for (id, _) in results {
        let summary = library.asset_summary(id).map_err(error)?;
        items.push(summary_view(summary));
    }
    Ok(items)
}

/// Loads the active embedding model and installs it into the library.
pub fn configure_embedder(library: &mut Library, registry: &ModelRegistry) -> Result<(), String> {
    let active = registry
        .list()
        .into_iter()
        .find(|model| model.active && model.state == picker_ml::ModelState::Ready);
    let Some(model) = active else {
        library.set_embedder(None).map_err(error)?;
        return Ok(());
    };
    let files = registry.load(&model.id).map_err(error)?;
    let embedder =
        picker_ml::ClipJapanese::from_paths(&files, &model.id, &model.version).map_err(error)?;
    library
        .set_embedder(Some(Arc::new(embedder)))
        .map_err(error)
}
