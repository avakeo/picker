use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use picker_core::{Library, ThumbnailState};
use picker_store::{LocationStatus, RootStatus};
use tauri::State;

use crate::contract::{
    AssetDetailView, AssetMetadataView, AssetPageView, AssetSearchRequest, AssetSummaryView,
    EventContract, FinderRevealRequest, IdRequest, IndexProgressChanged,
    IndexProgressChangedPayload, IndexStageProgress, IndexStatusView, LocationView, RootAddRequest,
    RootEnabledRequest, RootView, StageProgressView, ViewportRequest,
};

/// Shared application state managed by Tauri.
pub struct AppState {
    library: Mutex<Library>,
}

impl AppState {
    #[must_use]
    pub const fn new(library: Library) -> Self {
        Self {
            library: Mutex::new(library),
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
    }
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
