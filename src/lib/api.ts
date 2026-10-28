import { invoke } from "@tauri-apps/api/core";
import type {
  AssetDetailView,
  AssetPageView,
  AssetSearchRequest,
  FinderRevealRequest,
  IdRequest,
  IndexStatusView,
  RootAddRequest,
  RootEnabledRequest,
  RootView,
  TagAliasView,
  TagView,
  ViewportRequest,
  AssetSummaryView,
  ColorView,
  AlbumView,
  ModelProgressEvent,
  EmbeddingStatusView,
  ModelView,
  PaletteColorView,
  AlbumQueryView,
  CacheClearView
} from "./generated/ui-contract";

export type { AssetDetailView, AssetPageView, AssetSummaryView, IndexStatusView, RootView } from "./generated/ui-contract";

export function listRoots(): Promise<RootView[]> {
  return invoke("roots_list");
}

export function addRoot(path: string, displayName = ""): Promise<RootView> {
  const request: RootAddRequest = { path, displayName };
  return invoke("root_add", { request });
}

export function removeRoot(id: string): Promise<boolean> {
  const request: IdRequest = { id };
  return invoke("root_remove", { request });
}

export function setRootEnabled(id: string, enabled: boolean): Promise<RootView> {
  const request: RootEnabledRequest = { id, enabled };
  return invoke("root_set_enabled", { request });
}

export function rescanRoot(id: string): Promise<IndexStatusView> {
  const request: IdRequest = { id };
  return invoke("rescan_root", { request });
}

export function searchAssets(cursor: string | null, pageSize = 100): Promise<AssetPageView> {
  const request: AssetSearchRequest = { cursor, pageSize };
  return invoke("asset_search", { request });
}

export function assetDetail(id: string): Promise<AssetDetailView> {
  const request: IdRequest = { id };
  return invoke("asset_detail", { request });
}

export function revealAsset(assetId: string, locationId: string): Promise<string> {
  const request: FinderRevealRequest = { assetId, locationId };
  return invoke("finder_reveal", { request });
}

export function updateViewport(assetIds: string[]): Promise<number> {
  const request: ViewportRequest = { assetIds };
  return invoke("viewport_update", { request });
}

export function indexStatus(): Promise<IndexStatusView> {
  return invoke("index_status");
}

export function pauseIndex(): Promise<IndexStatusView> {
  return invoke("pause_index");
}

export function resumeIndex(): Promise<IndexStatusView> {
  return invoke("resume_index");
}

export function thumbnailUrl(assetId: string, size: "grid" | "detail"): string {
  const origin = navigator.userAgent.includes("Windows")
    ? "http://asset.localhost"
    : "asset://localhost";
  return `${origin}/thumbnail/${encodeURIComponent(assetId)}?size=${size}`;
}

export type {
  AlbumView,
  AssetTagView,
  ColorView,
  ModelProgressEvent,
  ModelView,
  PaletteColorView,
  EmbeddingStatusView,
  TagAliasView,
  TagView
} from "./generated/ui-contract";

export function listTags(): Promise<TagView[]> {
  return invoke("tags_list");
}

export function createTag(
  canonicalName: string,
  displayNameJa: string,
  parentId: string | null = null,
  color: string | null = null
): Promise<TagView> {
  const request = { canonicalName, displayNameJa, parentId, color };
  return invoke("tag_create", { request });
}

export function renameTag(id: string, canonicalName: string, displayNameJa: string): Promise<TagView> {
  const request = { id, canonicalName, displayNameJa };
  return invoke("tag_rename", { request });
}

export function deleteTag(id: string): Promise<boolean> {
  return invoke("tag_delete", { request: { id } });
}

export function setTagParent(id: string, parentId: string | null): Promise<TagView> {
  const request = { id, parentId };
  return invoke("tag_set_parent", { request });
}

export function mergeTags(sourceId: string, targetId: string): Promise<TagView> {
  const request = { sourceId, targetId };
  return invoke("tag_merge", { request });
}

export function addTagAlias(tagId: string, alias: string): Promise<void> {
  return invoke("tag_add_alias", { request: { tagId, alias } });
}

export function removeTagAlias(id: string): Promise<boolean> {
  return invoke("tag_remove_alias", { request: { id } });
}

export function assignTag(assetIds: string[], tagId: string): Promise<number> {
  return invoke("tag_assign", { request: { assetIds, tagId } });
}

export function removeTag(assetIds: string[], tagId: string): Promise<number> {
  return invoke("tag_remove", { request: { assetIds, tagId } });
}

export function filterAssets(
  cursor: string | null,
  pageSize: number,
  filter: {
    tagsAll: string[];
    tagsAny: string[];
    tagsNone: string[];
    includeSuggested: boolean;
    colors: string[];
    colorScope: "main" | "any";
    formats: string[];
    minWidth: number | null;
    minHeight: number | null;
  }
): Promise<AssetPageView> {
  return invoke("asset_filter", {
    request: {
      cursor,
      pageSize,
      ...filter,
      capturedFrom: null,
      capturedTo: null
    }
  });
}

export function listPalette(): Promise<PaletteColorView[]> {
  return invoke("palette_list");
}

export function listDuplicates(): Promise<AssetSummaryView[]> {
  return invoke("duplicates_list");
}

export function listTagAliases(tagId: string): Promise<TagAliasView[]> {
  return invoke("tag_aliases", { request: { id: tagId } });
}

export function setColors(assetId: string, colors: { hex: string; ratio: number }[]): Promise<ColorView[]> {
  return invoke("color_set", { request: { assetId, colors } });
}

export function resetColors(id: string): Promise<ColorView[]> {
  return invoke("color_reset", { request: { id } });
}

export function listAlbums(): Promise<AlbumView[]> {
  return invoke("albums_list");
}

export function createAlbum(name: string): Promise<AlbumView> {
  return invoke("album_create", { request: { name } });
}

export function renameAlbum(id: string, name: string): Promise<AlbumView> {
  return invoke("album_rename", { request: { id, name } });
}

export function deleteAlbum(id: string): Promise<boolean> {
  return invoke("album_delete", { request: { id } });
}

export function addAlbumAssets(albumId: string, assetIds: string[]): Promise<number> {
  return invoke("album_add_assets", { request: { albumId, assetIds } });
}

export function removeAlbumAssets(albumId: string, assetIds: string[]): Promise<number> {
  return invoke("album_remove_assets", { request: { albumId, assetIds } });
}

export function reorderAlbum(albumId: string, assetIds: string[]): Promise<void> {
  return invoke("album_reorder", { request: { albumId, assetIds } });
}

export function createAutoAlbum(name: string, query: AlbumQueryView): Promise<AlbumView> {
  return invoke("album_create_auto", { request: { name, query } });
}

export function setAlbumQuery(id: string, query: AlbumQueryView): Promise<AlbumView> {
  return invoke("album_set_query", { request: { id, query } });
}

export function autoAlbumPage(
  id: string,
  cursor: string | null,
  pageSize: number
): Promise<AssetPageView> {
  return invoke("album_auto_page", { request: { id, cursor, pageSize } });
}

export function listAlbumAssets(id: string): Promise<AssetSummaryView[]> {
  return invoke("album_assets", { request: { id } });
}

export function listModels(): Promise<ModelView[]> {
  return invoke("models_list");
}

export function installModel(id: string): Promise<void> {
  return invoke("model_install", { request: { id } });
}

export function activateModel(id: string): Promise<void> {
  return invoke("model_activate", { request: { id } });
}

export function removeModel(id: string, version: string): Promise<void> {
  return invoke("model_remove", { request: { id: `${id}@${version}` } });
}

export function embeddingStatus(): Promise<EmbeddingStatusView | null> {
  return invoke("embedding_status");
}

export function indexEmbeddings(limit: number): Promise<number> {
  return invoke("embedding_index", { request: { limit } });
}

export function semanticSearch(query: string, limit: number): Promise<AssetSummaryView[]> {
  return invoke("semantic_search", { request: { query, limit } });
}

export function similarAssets(id: string, limit: number): Promise<AssetSummaryView[]> {
  return invoke("similar_assets", { request: { id, limit } });
}

export function rejectTag(assetIds: string[], tagId: string): Promise<number> {
  return invoke("tag_reject", { request: { assetIds, tagId } });
}

export function autoTag(limit: number): Promise<number> {
  return invoke("auto_tag", { request: { limit } });
}

export function convertTagToCustom(id: string): Promise<TagView> {
  return invoke("tag_convert_to_custom", { request: { id } });
}

export function customSuggest(tagId: string, limit: number): Promise<number> {
  return invoke("custom_suggest", { request: { tagId, limit } });
}

export function trainCustomTag(tagId: string): Promise<number> {
  return invoke("custom_train", { request: { tagId } });
}

export function reviewCandidates(tagId: string, limit = 20, highRatio = 0.5): Promise<AssetSummaryView[]> {
  return invoke("review_candidates", { request: { tagId, limit, highRatio } });
}

export function confirmReview(tagId: string, keepAssetIds: string[], rejectAssetIds: string[]): Promise<string[]> {
  return invoke("review_confirm", { request: { tagId, keepAssetIds, rejectAssetIds } });
}

export function undoFeedback(eventId: string): Promise<void> {
  return invoke("feedback_undo", { request: { eventId } });
}

export function exportPortable(path: string): Promise<number> {
  return invoke("portable_export", { request: { path } });
}

export function importPortable(path: string): Promise<number> {
  return invoke("portable_import", { request: { path } });
}

export function getSetting(key: string): Promise<string | null> {
  return invoke("setting_get", { request: { key } });
}

export function setSetting(key: string, value: string): Promise<void> {
  return invoke("setting_set", { request: { key, value } });
}

export function clearCache(target: "thumbnails" | "embeddings" | "all"): Promise<CacheClearView> {
  return invoke("cache_clear", { request: { target } });
}

export function exportXmp(assetId: string): Promise<number> {
  return invoke("xmp_export", { request: { id: assetId } });
}
