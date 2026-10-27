//! Typed UI contract shared by Tauri commands, events, and generated bindings.

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS};

/// Marker implemented by every UI command contract.
pub trait CommandContract {
    /// Stable command name exposed through Tauri.
    const NAME: &'static str;
    /// JSON request sent by the UI.
    type Request;
    /// Successful JSON response returned to the UI.
    type Response;
}

/// Marker implemented by every event contract.
pub trait EventContract {
    /// Stable event name emitted through Tauri.
    const NAME: &'static str;
    /// JSON payload delivered to the UI.
    type Payload;
}

/// Empty request used by commands without arguments.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct GetAppInfoRequest {}

/// A license displayed by the application information screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct LicenseInfo {
    /// Component or model name.
    pub name: String,
    /// SPDX license expression.
    pub license: String,
}

/// Response for the app information command in UI contract section 3.7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AppInfo {
    /// Application version.
    pub version: String,
    /// Licenses for application dependencies.
    pub dependency_licenses: Vec<LicenseInfo>,
    /// Licenses for installed models.
    pub model_licenses: Vec<LicenseInfo>,
}

/// Contract marker for `get_app_info`.
pub struct GetAppInfo;

impl CommandContract for GetAppInfo {
    const NAME: &'static str = "get_app_info";
    type Request = GetAppInfoRequest;
    type Response = AppInfo;
}

/// Progress for one indexing stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct IndexStageProgress {
    /// Stable stage identifier.
    pub stage: String,
    /// Number of completed items.
    pub completed: u32,
    /// Number of queued items.
    pub pending: u32,
    /// Number of failed items.
    pub failed: u32,
}

/// Payload for the index progress event in UI contract section 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct IndexProgressChangedPayload {
    /// Current per-stage progress.
    pub stages: Vec<IndexStageProgress>,
    /// Whether indexing is paused.
    pub paused: bool,
}

/// Contract marker for `index-progress-changed`.
pub struct IndexProgressChanged;

impl EventContract for IndexProgressChanged {
    const NAME: &'static str = "index-progress-changed";
    type Payload = IndexProgressChangedPayload;
}

/// Generates the committed TypeScript contract from Rust source types.
#[must_use]
pub fn generated_typescript() -> String {
    let config = Config::default();
    let declarations = [
        GetAppInfoRequest::decl(&config),
        LicenseInfo::decl(&config),
        AppInfo::decl(&config),
        IndexStageProgress::decl(&config),
        IndexProgressChangedPayload::decl(&config),
        RootView::decl(&config),
        AssetSummaryView::decl(&config),
        AssetPageView::decl(&config),
        LocationView::decl(&config),
        AssetMetadataView::decl(&config),
        AssetDetailView::decl(&config),
        StageProgressView::decl(&config),
        IndexStatusView::decl(&config),
        RootAddRequest::decl(&config),
        IdRequest::decl(&config),
        RootEnabledRequest::decl(&config),
        AssetSearchRequest::decl(&config),
        ViewportRequest::decl(&config),
        FinderRevealRequest::decl(&config),
        TagView::decl(&config),
        TagAliasView::decl(&config),
        AssetTagView::decl(&config),
        TagCreateRequest::decl(&config),
        TagRenameRequest::decl(&config),
        TagParentRequest::decl(&config),
        TagMergeRequest::decl(&config),
        TagAliasRequest::decl(&config),
        TagAliasDeleteRequest::decl(&config),
        TagAssignRequest::decl(&config),
        TagRemoveRequest::decl(&config),
        AssetFilterRequest::decl(&config),
        ColorView::decl(&config),
        ColorInput::decl(&config),
        ColorSetRequest::decl(&config),
        AlbumView::decl(&config),
        AlbumCreateRequest::decl(&config),
        AlbumRenameRequest::decl(&config),
        AlbumAssetsRequest::decl(&config),
        AlbumReorderRequest::decl(&config),
        ModelView::decl(&config),
        ModelIdRequest::decl(&config),
        ModelProgressEvent::decl(&config),
        EmbeddingStatusView::decl(&config),
        EmbeddingIndexRequest::decl(&config),
        SemanticSearchRequest::decl(&config),
        SimilarAssetsRequest::decl(&config),
    ]
    .map(|declaration| {
        let declaration = declaration
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        format!("export {declaration}")
    });

    format!(
        "// This file is generated by `cargo run -p picker-app --bin generate-types`.\n\
         // Do not edit it directly.\n\n\
         {}\n\n\
         export const COMMANDS = {{
           getAppInfo: \"{}\",
           rootsList: \"roots_list\",
           rootAdd: \"root_add\",
           rootRemove: \"root_remove\",
           rootSetEnabled: \"root_set_enabled\",
           rescanRoot: \"rescan_root\",
           assetSearch: \"asset_search\",
           assetDetail: \"asset_detail\",
           finderReveal: \"finder_reveal\",
           viewportUpdate: \"viewport_update\",
           indexStatus: \"index_status\",
           pauseIndex: \"pause_index\",
           resumeIndex: \"resume_index\",
           tagsList: \"tags_list\",
           tagCreate: \"tag_create\",
           tagRename: \"tag_rename\",
           tagDelete: \"tag_delete\",
           tagSetParent: \"tag_set_parent\",
           tagMerge: \"tag_merge\",
           tagAliases: \"tag_aliases\",
           tagAddAlias: \"tag_add_alias\",
           tagRemoveAlias: \"tag_remove_alias\",
           tagAssign: \"tag_assign\",
           tagRemove: \"tag_remove\",
           assetFilter: \"asset_filter\",
           duplicatesList: \"duplicates_list\",
           colorSet: \"color_set\",
           colorReset: \"color_reset\",
           albumsList: \"albums_list\",
           albumCreate: \"album_create\",
           albumRename: \"album_rename\",
           albumDelete: \"album_delete\",
           albumAddAssets: \"album_add_assets\",
           albumRemoveAssets: \"album_remove_assets\",
           albumReorder: \"album_reorder\",
           albumAssets: \"album_assets\",
           modelsList: \"models_list\",
           modelInstall: \"model_install\",
           modelActivate: \"model_activate\",
           modelRemove: \"model_remove\",
           embeddingStatus: \"embedding_status\",
           embeddingIndex: \"embedding_index\",
           semanticSearch: \"semantic_search\",
           similarAssets: \"similar_assets\",
         }} as const;\n\
         export const EVENTS = {{ indexProgressChanged: \"{}\", modelProgressChanged: \"{}\" }} as const;\n",
        declarations.join("\n\n"),
        GetAppInfo::NAME,
        IndexProgressChanged::NAME,
        ModelProgressChanged::NAME,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_names_are_stable_and_distinct() {
        assert_eq!(GetAppInfo::NAME, "get_app_info");
        assert_eq!(IndexProgressChanged::NAME, "index-progress-changed");
        assert_ne!(GetAppInfo::NAME, IndexProgressChanged::NAME);
    }

    #[test]
    fn event_payload_uses_camel_case_and_rejects_unknown_fields() {
        let payload = IndexProgressChangedPayload {
            stages: vec![IndexStageProgress {
                stage: "thumbnail".into(),
                completed: 2,
                pending: 1,
                failed: 0,
            }],
            paused: false,
        };
        let value = serde_json::to_value(payload).expect("event payload should serialize");
        assert_eq!(value["stages"][0]["completed"], 2);
        assert!(value["stages"][0].get("completed_items").is_none());

        let invalid = r#"{"stages":[],"paused":false,"extra":true}"#;
        assert!(serde_json::from_str::<IndexProgressChangedPayload>(invalid).is_err());
    }

    #[test]
    fn committed_typescript_matches_rust_types() {
        let committed = include_str!("../../src/lib/generated/ui-contract.ts");
        assert_eq!(committed, generated_typescript());
    }
}

/// A registered root in the folder tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct RootView {
    pub id: String,
    pub path: String,
    pub display_name: String,
    pub enabled: bool,
    pub online: bool,
    pub image_count: u32,
}

/// A grid item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetSummaryView {
    pub id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
    pub captured_at: Option<i64>,
    pub thumbnail_state: String,
    pub location_status: String,
}

/// A page of grid items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetPageView {
    pub items: Vec<AssetSummaryView>,
    pub next_cursor: Option<String>,
    pub total: u32,
    pub total_is_exact: bool,
}

/// One physical location of an asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct LocationView {
    pub id: String,
    pub root_id: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub status: String,
}

/// EXIF cache shown by the detail panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetMetadataView {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub iso: Option<u32>,
    pub focal_length_mm: Option<f64>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
}

/// Detail panel response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetDetailView {
    pub summary: AssetSummaryView,
    pub file_size: u64,
    pub locations: Vec<LocationView>,
    pub metadata: Option<AssetMetadataView>,
    pub tags: Vec<AssetTagView>,
    pub colors: Vec<ColorView>,
}

/// Per-stage indexing progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct StageProgressView {
    pub stage: String,
    pub completed: u32,
    pub pending: u32,
    pub failed: u32,
}

/// Index status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct IndexStatusView {
    pub stages: Vec<StageProgressView>,
    pub paused: bool,
    pub total_assets: u32,
}

/// Request for `root_add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct RootAddRequest {
    pub path: String,
    pub display_name: String,
}

/// Request carrying one opaque id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct IdRequest {
    pub id: String,
}

/// Request to enable or disable a root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct RootEnabledRequest {
    pub id: String,
    pub enabled: bool,
}

/// Request for `asset_search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetSearchRequest {
    pub cursor: Option<String>,
    pub page_size: u32,
}

/// Request for `viewport_update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ViewportRequest {
    pub asset_ids: Vec<String>,
}

/// Request for `finder_reveal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct FinderRevealRequest {
    pub asset_id: String,
    pub location_id: String,
}

/// A tag definition with its confirmed asset count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagView {
    pub id: String,
    pub canonical_name: String,
    pub display_name_ja: String,
    pub display_name_en: Option<String>,
    pub parent_id: Option<String>,
    pub color: Option<String>,
    pub origin: String,
    pub asset_count: u32,
}

/// An alias attached to a tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagAliasView {
    pub id: String,
    pub tag_id: String,
    pub alias: String,
}

/// A tag shown in the detail panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetTagView {
    pub tag_id: String,
    pub canonical_name: String,
    pub display_name_ja: String,
    pub state: String,
    pub source: String,
    pub confidence: Option<f64>,
}

/// Request for `tag_create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagCreateRequest {
    pub canonical_name: String,
    pub display_name_ja: String,
    pub parent_id: Option<String>,
    pub color: Option<String>,
}

/// Request for `tag_rename`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagRenameRequest {
    pub id: String,
    pub canonical_name: String,
    pub display_name_ja: String,
}

/// Request for `tag_set_parent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagParentRequest {
    pub id: String,
    pub parent_id: Option<String>,
}

/// Request for `tag_merge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagMergeRequest {
    pub source_id: String,
    pub target_id: String,
}

/// Request for `tag_add_alias`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagAliasRequest {
    pub tag_id: String,
    pub alias: String,
}

/// Request for `tag_remove_alias`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagAliasDeleteRequest {
    pub id: String,
}

/// Request for assigning a confirmed tag to one or more assets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagAssignRequest {
    pub asset_ids: Vec<String>,
    pub tag_id: String,
}

/// Request for removing a user tag from one or more assets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct TagRemoveRequest {
    pub asset_ids: Vec<String>,
    pub tag_id: String,
}

/// Request for `asset_filter`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AssetFilterRequest {
    pub cursor: Option<String>,
    pub page_size: u32,
    pub tag_ids: Vec<String>,
    pub tag_mode: String,
    pub include_suggested: bool,
    pub formats: Vec<String>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub captured_from: Option<i64>,
    pub captured_to: Option<i64>,
}

/// An extracted or user-edited dominant color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ColorView {
    pub rank: u8,
    pub hex: String,
    pub ratio: f64,
    pub source: String,
}

/// One color supplied by the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ColorInput {
    pub hex: String,
    pub ratio: f64,
}

/// Request for `color_set`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ColorSetRequest {
    pub asset_id: String,
    pub colors: Vec<ColorInput>,
}

/// An album shown in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AlbumView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub asset_count: u32,
}

/// Request for `album_create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AlbumCreateRequest {
    pub name: String,
}

/// Request for `album_rename`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AlbumRenameRequest {
    pub id: String,
    pub name: String,
}

/// Request carrying an album id and assets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AlbumAssetsRequest {
    pub album_id: String,
    pub asset_ids: Vec<String>,
}

/// Request carrying a complete album order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct AlbumReorderRequest {
    pub album_id: String,
    pub asset_ids: Vec<String>,
}

/// An embedding model known to the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ModelView {
    pub id: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    pub size: u64,
    pub state: String,
    pub active: bool,
}

/// Request naming a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ModelIdRequest {
    pub id: String,
}

/// Contract marker for the model progress event.
pub struct ModelProgressChanged;

impl EventContract for ModelProgressChanged {
    const NAME: &'static str = "model-progress";
    type Payload = ModelProgressEvent;
}

/// Payload for the model download progress event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ModelProgressEvent {
    pub model_id: String,
    pub version: String,
    pub file: String,
    pub file_index: u32,
    pub file_count: u32,
    pub file_downloaded: u64,
    pub file_total: u64,
    pub total_downloaded: u64,
    pub total_size: u64,
}

/// Progress of the active embedding model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct EmbeddingStatusView {
    pub model_id: String,
    pub model_version: String,
    pub dimensions: u32,
    pub indexed: u32,
    pub missing: u32,
}

/// Request to process a bounded batch of embeddings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct EmbeddingIndexRequest {
    pub limit: u32,
}

/// Request for a natural-language search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SemanticSearchRequest {
    pub query: String,
    pub limit: u32,
}

/// Request for similar images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SimilarAssetsRequest {
    pub id: String,
    pub limit: u32,
}
