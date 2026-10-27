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
  ViewportRequest
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
