<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import AssetGrid from "./lib/AssetGrid.svelte";
  import {
    addRoot,
    assetDetail,
    indexStatus,
    listRoots,
    pauseIndex,
    removeRoot,
    rescanRoot,
    resumeIndex,
    revealAsset,
    searchAssets,
    setRootEnabled,
    thumbnailUrl,
    updateViewport,
    type AssetDetailView,
    type AssetPageView,
    type AssetSummaryView,
    type IndexStatusView,
    type RootView
  } from "./lib/api";

  let roots: RootView[] = [];
  let assets: AssetSummaryView[] = [];
  let nextCursor: string | null = null;
  let total = 0;
  let detail: AssetDetailView | null = null;
  let selectedId = "";
  let status: IndexStatusView | null = null;
  let folderPath = "";
  let errorMessage = "";
  let busy = false;
  let loadingMore = false;
  let priorityTimer: ReturnType<typeof setTimeout> | undefined;

  async function guard(action: () => Promise<void>) {
    busy = true;
    errorMessage = "";
    try {
      await action();
    } catch (error) {
      errorMessage = String(error);
    } finally {
      busy = false;
    }
  }

  async function refreshRoots() {
    roots = await listRoots();
  }

  async function refreshPage(reset: boolean) {
    const page: AssetPageView = reset
      ? await searchAssets(null)
      : await searchAssets(nextCursor);
    assets = reset ? page.items : [...assets, ...page.items];
    nextCursor = page.nextCursor;
    total = page.total;
  }

  async function refreshStatus() {
    status = await indexStatus();
  }

  async function refreshAll() {
    await guard(async () => {
      await Promise.all([refreshRoots(), refreshPage(true), refreshStatus()]);
    });
  }

  async function handleAddRoot() {
    const path = folderPath.trim();
    if (!path) return;
    await guard(async () => {
      await addRoot(path);
      folderPath = "";
      await refreshRoots();
    });
  }

  async function handleRemoveRoot(root: RootView) {
    await guard(async () => {
      await removeRoot(root.id);
      selectedId = "";
      detail = null;
      await refreshAll();
    });
  }

  async function handleRescan(root: RootView) {
    await guard(async () => {
      status = await rescanRoot(root.id);
      await Promise.all([refreshRoots(), refreshPage(true)]);
    });
  }

  async function handleToggleRoot(root: RootView) {
    await guard(async () => {
      await setRootEnabled(root.id, !root.enabled);
      await refreshRoots();
    });
  }

  async function handleSelect(asset: AssetSummaryView) {
    selectedId = asset.id;
    await guard(async () => {
      detail = await assetDetail(asset.id);
    });
  }

  async function handleReveal() {
    const selected = detail;
    const location = selected?.locations[0];
    if (!selected || !location) return;
    await guard(async () => {
      await revealAsset(selected.summary.id, location.id);
    });
  }

  async function loadMore() {
    if (loadingMore || !nextCursor) return;
    loadingMore = true;
    await guard(() => refreshPage(false));
    loadingMore = false;
  }

  function handleViewportPriority(assetIds: string[]) {
    if (priorityTimer) clearTimeout(priorityTimer);
    priorityTimer = setTimeout(() => {
      if (assetIds.length > 0) void updateViewport(assetIds).catch(() => 0);
    }, 250);
  }

  async function togglePause() {
    await guard(async () => {
      status = status?.paused ? await resumeIndex() : await pauseIndex();
    });
  }

  onMount(() => {
    void refreshAll();
    const unlisten = listen("index-progress-changed", () => {
      void refreshStatus();
    });
    return () => {
      if (priorityTimer) clearTimeout(priorityTimer);
      void unlisten.then((stop) => stop());
    };
  });
</script>

<svelte:head>
  <title>Picker</title>
</svelte:head>

<div class="app">
  <aside>
    <h1>Picker</h1>
    <form class="add-root" onsubmit={(event) => { event.preventDefault(); void handleAddRoot(); }}>
      <label for="folder-path">フォルダを追加</label>
      <input id="folder-path" bind:value={folderPath} placeholder="/Users/me/Pictures" />
      <button type="submit" disabled={busy}>追加</button>
    </form>
    {#if roots.length === 0}
      <p class="muted">フォルダが登録されていません。</p>
    {/if}
    <ul class="roots">
      {#each roots as root (root.id)}
        <li class:disabled={!root.enabled}>
          <div class="root-name" title={root.path}>{root.displayName}</div>
          <div class="muted">{root.path}</div>
          <div class="root-meta">{root.imageCount} 枚 · {root.online ? "オンライン" : "オフライン"}</div>
          <div class="root-actions">
            <button type="button" disabled={busy} onclick={() => void handleRescan(root)}>再スキャン</button>
            <button type="button" disabled={busy} onclick={() => void handleToggleRoot(root)}>
              {root.enabled ? "無効化" : "有効化"}
            </button>
            <button type="button" disabled={busy} onclick={() => void handleRemoveRoot(root)}>削除</button>
          </div>
        </li>
      {/each}
    </ul>
  </aside>

  <main>
    <div class="toolbar">
      <div><strong>{total}</strong> 枚</div>
      {#if status}
        <div class="stages">
          {#each status.stages.filter((stage) => stage.completed + stage.pending + stage.failed > 0) as stage (stage.stage)}
            <span>{stage.stage}: {stage.completed} 完了 / {stage.pending} 待ち / {stage.failed} 失敗</span>
          {/each}
        </div>
        <button type="button" disabled={busy} onclick={() => void togglePause()}>
          {status.paused ? "再開" : "一時停止"}
        </button>
      {/if}
      <button type="button" disabled={busy} onclick={() => void refreshAll()}>更新</button>
    </div>

    {#if errorMessage}<p class="error" role="alert">{errorMessage}</p>{/if}

    <div class="content">
      <div class="grid-area">
        {#if assets.length === 0}
          <p class="muted empty">画像がありません。フォルダを追加して再スキャンしてください。</p>
        {:else}
          <AssetGrid
            items={assets}
            {selectedId}
            onSelect={(asset) => void handleSelect(asset)}
            onNearEnd={() => void loadMore()}
            onVisible={handleViewportPriority}
            thumbnail={(id) => thumbnailUrl(id, "grid")}
          />
          {#if nextCursor}
            <button type="button" class="more" disabled={busy} onclick={() => void guard(() => refreshPage(false))}>
              さらに読み込む
            </button>
          {/if}
        {/if}
      </div>

      <section class="detail">
        {#if detail}
          <img class="preview" src={thumbnailUrl(detail.summary.id, "detail")} alt="" />
          <dl>
            <dt>ID</dt><dd>{detail.summary.id}</dd>
            <dt>形式</dt><dd>{detail.summary.format ?? "不明"}</dd>
            <dt>サイズ</dt><dd>{detail.summary.width ?? "?"} × {detail.summary.height ?? "?"}</dd>
            <dt>ファイル</dt><dd>{detail.fileSize.toLocaleString()} bytes</dd>
            <dt>撮影日時</dt><dd>{detail.summary.capturedAt ? new Date(Number(detail.summary.capturedAt) * 1000).toLocaleString() : "不明"}</dd>
            {#if detail.metadata?.cameraMake}<dt>機材</dt><dd>{detail.metadata.cameraMake} {detail.metadata.cameraModel ?? ""}</dd>{/if}
            {#if detail.metadata?.lens}<dt>レンズ</dt><dd>{detail.metadata.lens}</dd>{/if}
            {#if detail.metadata?.iso}<dt>ISO</dt><dd>{detail.metadata.iso}</dd>{/if}
          </dl>
          <h2>保存場所</h2>
          <ul class="locations">
            {#each detail.locations as location (location.id)}
              <li>
                <span class:missing={location.status !== "present"}>{location.absolutePath}</span>
                {#if location.status !== "present"}<em>({location.status})</em>{/if}
              </li>
            {/each}
          </ul>
          {#if detail.locations.length > 0}
            <button type="button" disabled={busy} onclick={() => void handleReveal()}>Finder で表示</button>
          {/if}
        {:else}
          <p class="muted">画像を選択してください。</p>
        {/if}
      </section>
    </div>
  </main>
</div>

<style>
  :global(html, body) { margin: 0; height: 100%; }
  :global(body) { background: #131313; color: #ececec; font-family: system-ui, sans-serif; }
  :global(*) { box-sizing: border-box; }
  .app { display: grid; grid-template-columns: 290px minmax(0, 1fr); height: 100vh; }
  aside { display: flex; flex-direction: column; gap: 12px; padding: 16px; overflow: auto; border-right: 1px solid #333; background: #1b1b1b; }
  main { display: grid; grid-template-rows: auto auto minmax(0, 1fr); min-width: 0; }
  h1 { margin: 0; font-size: 22px; }
  h2 { margin: 12px 0 0; font-size: 14px; }
  .add-root { display: grid; gap: 6px; }
  input { width: 100%; padding: 7px; border: 1px solid #555; border-radius: 5px; background: #232323; color: inherit; }
  button { padding: 6px 9px; border: 1px solid #555; border-radius: 5px; background: #2b2b2b; color: inherit; cursor: pointer; }
  button:disabled { cursor: wait; opacity: 0.6; }
  .roots { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .roots li { display: grid; gap: 3px; padding: 8px; border: 1px solid #3a3a3a; border-radius: 6px; }
  .roots li.disabled { opacity: 0.55; }
  .root-name { overflow: hidden; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .root-meta, .muted { color: #a3a3a3; font-size: 12px; }
  .root-actions { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 4px; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 10px 14px; border-bottom: 1px solid #333; }
  .stages { display: flex; flex: 1; flex-wrap: wrap; gap: 10px; color: #b6b6b6; font-size: 12px; }
  .error { margin: 0; padding: 8px 14px; background: #4a1d1d; color: #ffb4b4; }
  .content { display: grid; grid-template-columns: minmax(0, 1fr) 320px; min-height: 0; }
  .grid-area { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .grid-area :global(.virtual-grid) { flex: 1; }
  .empty { padding: 24px; }
  .more { margin: 0 10px 10px; }
  .detail { padding: 12px; overflow: auto; border-left: 1px solid #333; background: #181818; }
  .preview { display: block; width: 100%; max-height: 280px; object-fit: contain; background: #0e0e0e; }
  dl { display: grid; grid-template-columns: 70px minmax(0, 1fr); gap: 4px 8px; font-size: 12px; }
  dt { color: #999; }
  dd { margin: 0; overflow-wrap: anywhere; }
  .locations { display: grid; gap: 6px; padding: 0; list-style: none; font-size: 12px; }
  .locations span { overflow-wrap: anywhere; }
  .missing { color: #e08a8a; }
  @media (max-width: 900px) {
    .app { grid-template-columns: 220px minmax(0, 1fr); }
    .content { grid-template-columns: minmax(0, 1fr); }
    .detail { border-top: 1px solid #333; border-left: 0; }
  }
</style>
