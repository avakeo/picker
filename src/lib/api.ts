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
  AssetSummaryView
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

export type { AssetTagView, TagAliasView, TagView } from "./generated/ui-contract";

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
    tagIds: string[];
    tagMode: string;
    includeSuggested: boolean;
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

export function listDuplicates(): Promise<AssetSummaryView[]> {
  return invoke("duplicates_list");
}

export function listTagAliases(tagId: string): Promise<TagAliasView[]> {
  return invoke("tag_aliases", { request: { id: tagId } });
}
