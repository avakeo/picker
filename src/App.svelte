<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { listen } from "@tauri-apps/api/event";
  import AssetGrid from "./lib/AssetGrid.svelte";
  import {
    addAlbumAssets,
    addRoot,
    addTagAlias,
    assetDetail,
    assignTag,
    createAlbum,
    createTag,
    deleteAlbum,
    deleteTag,
    filterAssets,
    indexStatus,
    listAlbumAssets,
    listAlbums,
    listDuplicates,
    listRoots,
    listTagAliases,
    listTags,
    mergeTags,
    pauseIndex,
    removeAlbumAssets,
    removeRoot,
    removeTag,
    removeTagAlias,
    renameAlbum,
    renameTag,
    reorderAlbum,
    resetColors,
    setColors,
    rescanRoot,
    resumeIndex,
    revealAsset,
    searchAssets,
    setRootEnabled,
    setTagParent,
    thumbnailUrl,
    updateViewport,
    type AssetDetailView,
    type AssetPageView,
    type AlbumView,
    type AssetSummaryView,
    type ColorView,
    type IndexStatusView,
    type RootView,
    type TagAliasView,
    type TagView
  } from "./lib/api";

  type ViewMode = "all" | "filter" | "duplicates" | "album";
  type TagMode = "all" | "any" | "none";

  let roots: RootView[] = [];
  let tags: TagView[] = [];
  let albums: AlbumView[] = [];
  let currentAlbumId = "";
  let newAlbumName = "";
  let editingAlbumId = "";
  let editingAlbumName = "";
  let armedAlbumDeleteId = "";
  let assets: AssetSummaryView[] = [];
  let nextCursor: string | null = null;
  let total = 0;
  let detail: AssetDetailView | null = null;
  let selection = new SvelteSet<string>();
  let multiSelect = false;
  let viewMode: ViewMode = "all";
  let status: IndexStatusView | null = null;
  let folderPath = "";
  let errorMessage = "";
  let busy = false;
  let loadingMore = false;
  let priorityTimer: ReturnType<typeof setTimeout> | undefined;

  let selectedTagIds = new SvelteSet<string>();
  let tagMode: TagMode = "all";
  let includeSuggested = false;
  let formatFilter = "";
  let minWidth = "";
  let minHeight = "";
  let assignTagId = "";

  let newCanonical = "";
  let newDisplay = "";
  let newParent = "";
  let newColor = "";
  let editingTagId = "";
  let editingCanonical = "";
  let editingDisplay = "";
  let armedDeleteId = "";
  let mergeTargets: Record<string, string> = {};
  let aliasesByTag: Record<string, TagAliasView[]> = {};
  let aliasDrafts: Record<string, string> = {};
  let colorEditing = false;
  let colorDrafts: string[] = [];

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

  async function refreshTags() {
    tags = await listTags();
  }

  async function refreshAlbums() {
    albums = await listAlbums();
  }

  function filterArgs() {
    return {
      tagIds: [...selectedTagIds],
      tagMode,
      includeSuggested,
      formats: formatFilter ? [formatFilter] : [],
      minWidth: minWidth ? Number(minWidth) : null,
      minHeight: minHeight ? Number(minHeight) : null
    };
  }

  async function fetchPage(cursor: string | null): Promise<AssetPageView> {
    if (viewMode === "duplicates") {
      const items = await listDuplicates();
      return { items, nextCursor: null, total: items.length, totalIsExact: true };
    }
    if (viewMode === "album") {
      const items = await listAlbumAssets(currentAlbumId);
      return { items, nextCursor: null, total: items.length, totalIsExact: true };
    }
    if (viewMode === "filter") {
      return filterAssets(cursor, 100, filterArgs());
    }
    return searchAssets(cursor);
  }

  async function refreshPage(reset: boolean) {
    const page = await fetchPage(reset ? null : nextCursor);
    assets = reset ? page.items : [...assets, ...page.items];
    nextCursor = page.nextCursor;
    total = page.total;
  }

  async function refreshStatus() {
    status = await indexStatus();
  }

  async function refreshAll() {
    await guard(async () => {
      await Promise.all([
        refreshRoots(),
        refreshTags(),
        refreshAlbums(),
        refreshPage(true),
        refreshStatus()
      ]);
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
      clearSelection();
      await refreshAll();
    });
  }

  async function handleRescan(root: RootView) {
    await guard(async () => {
      status = await rescanRoot(root.id);
      await Promise.all([refreshRoots(), refreshPage(true), refreshTags()]);
    });
  }

  async function handleToggleRoot(root: RootView) {
    await guard(async () => {
      await setRootEnabled(root.id, !root.enabled);
      await refreshRoots();
    });
  }

  function clearSelection() {
    selection.clear();
    detail = null;
  }

  async function handleSelect(asset: AssetSummaryView) {
    if (multiSelect) {
      const next = new SvelteSet(selection);
      if (next.has(asset.id)) next.delete(asset.id);
      else next.add(asset.id);
      selection = next;
    } else {
      selection = new SvelteSet([asset.id]);
    }
    await guard(async () => {
      detail = await assetDetail(asset.id);
      colorEditing = false;
      colorDrafts = detail.colors.map((color) => color.hex);
    });
  }

  function beginColorEdit() {
    colorEditing = true;
    colorDrafts = detail?.colors.map((color) => color.hex) ?? [];
  }

  async function saveColors() {
    if (!detail) return;
    const hexes = colorDrafts.map((hex) => hex.trim()).filter(Boolean);
    const ratio = hexes.length > 0 ? 1 / hexes.length : 0;
    await guard(async () => {
      const colors = await setColors(detail!.summary.id, hexes.map((hex) => ({ hex, ratio })));
      detail = { ...detail!, colors };
      colorEditing = false;
    });
  }

  async function resetColorsToAuto() {
    if (!detail) return;
    await guard(async () => {
      const colors = await resetColors(detail!.summary.id);
      detail = { ...detail!, colors };
      colorEditing = false;
      colorDrafts = colors.map((color) => color.hex);
    });
  }

  function targetIds(): string[] {
    return [...selection];
  }

  async function loadMore() {
    if (loadingMore || !nextCursor || viewMode === "duplicates") return;
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

  async function applyFilter() {
    viewMode = "filter";
    clearSelection();
    await guard(() => refreshPage(true));
  }

  async function clearFilter() {
    selectedTagIds.clear();
    tagMode = "all";
    includeSuggested = false;
    formatFilter = "";
    minWidth = "";
    minHeight = "";
    viewMode = "all";
    clearSelection();
    await guard(() => refreshPage(true));
  }

  async function showAlbum(album: AlbumView) {
    currentAlbumId = album.id;
    viewMode = "album";
    clearSelection();
    await guard(() => refreshPage(true));
  }

  async function handleCreateAlbum() {
    const name = newAlbumName.trim();
    if (!name) return;
    await guard(async () => {
      await createAlbum(name);
      newAlbumName = "";
      await refreshAlbums();
    });
  }

  async function saveAlbumRename(albumId: string) {
    if (!editingAlbumName.trim()) return;
    await guard(async () => {
      await renameAlbum(albumId, editingAlbumName.trim());
      editingAlbumId = "";
      await refreshAlbums();
    });
  }

  async function handleDeleteAlbum(albumId: string) {
    if (armedAlbumDeleteId !== albumId) {
      armedAlbumDeleteId = albumId;
      return;
    }
    armedAlbumDeleteId = "";
    await guard(async () => {
      await deleteAlbum(albumId);
      if (currentAlbumId === albumId) {
        currentAlbumId = "";
        viewMode = "all";
      }
      await Promise.all([refreshAlbums(), refreshPage(true)]);
    });
  }

  async function addSelectionToAlbum(albumId: string) {
    const ids = targetIds();
    if (ids.length === 0) {
      errorMessage = "画像を選択してください。";
      return;
    }
    await guard(async () => {
      await addAlbumAssets(albumId, ids);
      await Promise.all([refreshAlbums(), refreshPage(true)]);
    });
  }

  async function addSelectionToCurrentAlbum() {
    if (currentAlbumId) await addSelectionToAlbum(currentAlbumId);
  }

  async function removeSelectionFromAlbum() {
    if (!currentAlbumId) return;
    const ids = targetIds();
    if (ids.length === 0) return;
    await guard(async () => {
      await removeAlbumAssets(currentAlbumId, ids);
      clearSelection();
      await Promise.all([refreshAlbums(), refreshPage(true)]);
    });
  }

  async function moveInAlbum(direction: -1 | 1) {
    if (!currentAlbumId || !detail) return;
    const ids = assets.map((asset) => asset.id);
    const index = ids.indexOf(detail.summary.id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= ids.length) return;
    [ids[index], ids[target]] = [ids[target], ids[index]];
    await guard(async () => {
      await reorderAlbum(currentAlbumId, ids);
      await refreshPage(true);
    });
  }

  async function showDuplicates() {
    viewMode = "duplicates";
    clearSelection();
    await guard(() => refreshPage(true));
  }

  function toggleFilterTag(tagId: string) {
    const next = new SvelteSet(selectedTagIds);
    if (next.has(tagId)) next.delete(tagId);
    else next.add(tagId);
    selectedTagIds = next;
  }

  async function handleAssignTag() {
    if (!assignTagId) return;
    const ids = targetIds();
    if (ids.length === 0) {
      errorMessage = "画像を選択してください。";
      return;
    }
    await guard(async () => {
      await assignTag(ids, assignTagId);
      if (detail && ids.includes(detail.summary.id)) {
        detail = await assetDetail(detail.summary.id);
      }
      await Promise.all([refreshTags(), refreshAlbums(), refreshPage(true)]);
    });
  }

  async function handleRemoveAssigned(tagId: string) {
    const ids = targetIds();
    if (ids.length === 0) return;
    await guard(async () => {
      await removeTag(ids, tagId);
      if (detail) detail = await assetDetail(detail.summary.id);
      await Promise.all([refreshTags(), refreshPage(true)]);
    });
  }

  async function handleCreateTag() {
    if (!newCanonical.trim()) return;
    await guard(async () => {
      const tag = await createTag(
        newCanonical.trim(),
        newDisplay.trim() || newCanonical.trim(),
        newParent || null,
        newColor || null
      );
      newCanonical = "";
      newDisplay = "";
      newParent = "";
      newColor = "";
      await refreshTags();
      assignTagId = tag.id;
    });
  }

  function beginRename(tag: TagView) {
    editingTagId = tag.id;
    editingCanonical = tag.canonicalName;
    editingDisplay = tag.displayNameJa;
  }

  async function saveRename(tagId: string) {
    if (!editingCanonical.trim()) return;
    await guard(async () => {
      await renameTag(tagId, editingCanonical.trim(), editingDisplay.trim() || editingCanonical.trim());
      editingTagId = "";
      await refreshTags();
    });
  }

  async function handleDeleteTag(tagId: string) {
    if (armedDeleteId !== tagId) {
      armedDeleteId = tagId;
      return;
    }
    armedDeleteId = "";
    await guard(async () => {
      await deleteTag(tagId);
      const next = new SvelteSet(selectedTagIds);
      next.delete(tagId);
      selectedTagIds = next;
      await Promise.all([refreshTags(), refreshPage(true)]);
    });
  }

  async function handleParentChange(tagId: string, parentId: string) {
    await guard(async () => {
      await setTagParent(tagId, parentId || null);
      await refreshTags();
    });
  }

  async function handleMerge(sourceId: string) {
    const targetId = mergeTargets[sourceId];
    if (!targetId || targetId === sourceId) return;
    await guard(async () => {
      await mergeTags(sourceId, targetId);
      const next = new SvelteSet(selectedTagIds);
      next.delete(sourceId);
      selectedTagIds = next;
      await Promise.all([refreshTags(), refreshPage(true)]);
    });
  }

  async function toggleAliases(tagId: string, open: boolean) {
    if (!open) return;
    await guard(async () => {
      aliasesByTag = { ...aliasesByTag, [tagId]: await listTagAliases(tagId) };
    });
  }

  async function handleAddAlias(tagId: string) {
    const alias = (aliasDrafts[tagId] ?? "").trim();
    if (!alias) return;
    await guard(async () => {
      await addTagAlias(tagId, alias);
      aliasDrafts = { ...aliasDrafts, [tagId]: "" };
      aliasesByTag = { ...aliasesByTag, [tagId]: await listTagAliases(tagId) };
    });
  }

  async function handleRemoveAlias(tagId: string, aliasId: string) {
    await guard(async () => {
      await removeTagAlias(aliasId);
      aliasesByTag = { ...aliasesByTag, [tagId]: await listTagAliases(tagId) };
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

  async function togglePause() {
    await guard(async () => {
      status = status?.paused ? await resumeIndex() : await pauseIndex();
    });
  }

  function toggleMultiSelect() {
    multiSelect = !multiSelect;
    if (!multiSelect && selection.size > 1) {
      const first = selection.values().next().value;
      selection = first ? new SvelteSet([first]) : new SvelteSet();
    }
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
    <form class="stack" onsubmit={(event) => { event.preventDefault(); void handleAddRoot(); }}>
      <label for="folder-path">フォルダを追加</label>
      <input id="folder-path" bind:value={folderPath} placeholder="/Users/me/Pictures" />
      <button type="submit" disabled={busy}>追加</button>
    </form>
    {#if roots.length === 0}<p class="muted">フォルダが登録されていません。</p>{/if}

    <section class="albums">
      <h2>アルバム</h2>
      <form class="inline" onsubmit={(event) => { event.preventDefault(); void handleCreateAlbum(); }}>
        <input bind:value={newAlbumName} placeholder="新規アルバム" aria-label="新規アルバム" />
        <button type="submit" disabled={busy}>作成</button>
      </form>
      <ul class="album-list">
        {#each albums as album (album.id)}
          <li class:current={album.id === currentAlbumId}>
            {#if editingAlbumId === album.id}
              <input bind:value={editingAlbumName} aria-label="アルバム名" />
              <button type="button" disabled={busy} onclick={() => void saveAlbumRename(album.id)}>保存</button>
              <button type="button" onclick={() => (editingAlbumId = "")}>取消</button>
            {:else}
              <button type="button" class="album-open" onclick={() => void showAlbum(album)}>
                {album.name} ({album.assetCount})
              </button>
              <button type="button" title="選択中の画像を追加" disabled={busy} onclick={() => void addSelectionToAlbum(album.id)}>＋</button>
              <button type="button" onclick={() => { editingAlbumId = album.id; editingAlbumName = album.name; }}>改名</button>
              <button type="button" class:danger={armedAlbumDeleteId === album.id} disabled={busy} onclick={() => void handleDeleteAlbum(album.id)}>
                {armedAlbumDeleteId === album.id ? "削除?" : "削除"}
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
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

    <section class="tags">
      <h2>タグフィルタ</h2>
      <div class="filter-row">
        <select bind:value={tagMode} aria-label="タグ条件">
          <option value="all">すべて含む (AND)</option>
          <option value="any">いずれか (OR)</option>
          <option value="none">含まない (NOT)</option>
        </select>
        <label class="check"><input type="checkbox" bind:checked={includeSuggested} /> 提案を含む</label>
      </div>
      <div class="filter-row">
        <select bind:value={formatFilter} aria-label="形式">
          <option value="">すべての形式</option>
          <option value="jpeg">JPEG</option>
          <option value="png">PNG</option>
          <option value="webp">WebP</option>
          <option value="heic">HEIC</option>
        </select>
        <input class="num" type="number" min="0" bind:value={minWidth} placeholder="最小幅" aria-label="最小幅" />
        <input class="num" type="number" min="0" bind:value={minHeight} placeholder="最小高" aria-label="最小高" />
      </div>
      <div class="filter-list">
        {#each tags as tag (tag.id)}
          <label class="check tag-filter">
            <input
              type="checkbox"
              checked={selectedTagIds.has(tag.id)}
              onchange={() => toggleFilterTag(tag.id)}
            />
            {tag.displayNameJa} <span class="muted">({tag.assetCount})</span>
          </label>
        {/each}
      </div>
      <div class="filter-row">
        <button type="button" disabled={busy} onclick={() => void applyFilter()}>絞り込む</button>
        <button type="button" disabled={busy} onclick={() => void clearFilter()}>解除</button>
        <button type="button" disabled={busy} onclick={() => void showDuplicates()}>重複</button>
      </div>
    </section>

    <details class="tag-manager">
      <summary>タグ管理</summary>
      <form class="stack" onsubmit={(event) => { event.preventDefault(); void handleCreateTag(); }}>
        <input bind:value={newCanonical} placeholder="正規名 (例: sunset)" aria-label="正規名" />
        <input bind:value={newDisplay} placeholder="表示名 (例: 夕日)" aria-label="表示名" />
        <select bind:value={newParent} aria-label="親タグ">
          <option value="">親なし</option>
          {#each tags as tag (tag.id)}<option value={tag.id}>{tag.displayNameJa}</option>{/each}
        </select>
        <input bind:value={newColor} placeholder="#RRGGBB (任意)" aria-label="色" />
        <button type="submit" disabled={busy}>タグを作成</button>
      </form>
      <ul class="tag-admin">
        {#each tags as tag (tag.id)}
          <li>
            <div class="tag-title">
              {#if tag.color}<span class="swatch" style={`background:${tag.color}`}></span>{/if}
              {#if editingTagId === tag.id}
                <input bind:value={editingCanonical} aria-label="正規名" />
                <input bind:value={editingDisplay} aria-label="表示名" />
                <button type="button" disabled={busy} onclick={() => void saveRename(tag.id)}>保存</button>
                <button type="button" onclick={() => (editingTagId = "")}>取消</button>
              {:else}
                <strong>{tag.displayNameJa}</strong>
                <span class="muted">{tag.canonicalName} · {tag.assetCount}</span>
                <button type="button" onclick={() => beginRename(tag)}>改名</button>
                <button type="button" class:danger={armedDeleteId === tag.id} disabled={busy} onclick={() => void handleDeleteTag(tag.id)}>
                  {armedDeleteId === tag.id ? "本当に削除" : "削除"}
                </button>
              {/if}
            </div>
            <div class="inline">
              <select value={tag.parentId ?? ""} aria-label="親タグ変更" onchange={(event) => void handleParentChange(tag.id, event.currentTarget.value)}>
                <option value="">親なし</option>
                {#each tags.filter((candidate) => candidate.id !== tag.id) as candidate (candidate.id)}
                  <option value={candidate.id}>{candidate.displayNameJa}</option>
                {/each}
              </select>
              <select bind:value={mergeTargets[tag.id]} aria-label="統合先">
                <option value="">統合先</option>
                {#each tags.filter((candidate) => candidate.id !== tag.id) as candidate (candidate.id)}
                  <option value={candidate.id}>{candidate.displayNameJa}</option>
                {/each}
              </select>
              <button type="button" disabled={busy || !mergeTargets[tag.id]} onclick={() => void handleMerge(tag.id)}>統合</button>
            </div>
            <details ontoggle={(event) => void toggleAliases(tag.id, event.currentTarget.open)}>
              <summary>別名</summary>
              <div class="inline">
                <input bind:value={aliasDrafts[tag.id]} placeholder="別名" aria-label="別名" />
                <button type="button" disabled={busy} onclick={() => void handleAddAlias(tag.id)}>追加</button>
              </div>
              <ul class="aliases">
                {#each aliasesByTag[tag.id] ?? [] as alias (alias.id)}
                  <li>{alias.alias} <button type="button" onclick={() => void handleRemoveAlias(tag.id, alias.id)}>×</button></li>
                {/each}
              </ul>
            </details>
          </li>
        {/each}
      </ul>
    </details>
  </aside>

  <main>
    <div class="toolbar">
      <div>
        <strong>{total}</strong> 枚
        {viewMode === "filter" ? "(絞り込み中)" : ""}
        {viewMode === "duplicates" ? "(重複)" : ""}
        {viewMode === "album" ? `(アルバム: ${albums.find((album) => album.id === currentAlbumId)?.name ?? ""})` : ""}
      </div>
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
      {#if viewMode === "album"}
        <button type="button" disabled={busy} onclick={() => void addSelectionToCurrentAlbum()}>アルバムに追加</button>
        <button type="button" disabled={busy} onclick={() => void removeSelectionFromAlbum()}>アルバムから外す</button>
      {/if}
      <button type="button" class:active={multiSelect} onclick={toggleMultiSelect}>
        複数選択 {multiSelect ? `(${selection.size})` : ""}
      </button>
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
            selectedIds={selection}
            onSelect={(asset) => void handleSelect(asset)}
            onNearEnd={() => void loadMore()}
            onVisible={handleViewportPriority}
            thumbnail={(id) => thumbnailUrl(id, "grid")}
          />
          {#if nextCursor}
            <button type="button" class="more" disabled={busy} onclick={() => void loadMore()}>
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

          <h2>タグ</h2>
          <div class="chips">
            {#each detail.tags as assigned (assigned.tagId)}
              <span class="chip" class:suggested={assigned.state === "suggested"}>
                {assigned.displayNameJa}
                <em>{assigned.state}/{assigned.source}</em>
                <button type="button" aria-label={`${assigned.displayNameJa} を外す`} onclick={() => void handleRemoveAssigned(assigned.tagId)}>×</button>
              </span>
            {/each}
          </div>
          <div class="inline">
            <select bind:value={assignTagId} aria-label="付与するタグ">
              <option value="">タグを選択</option>
              {#each tags as tag (tag.id)}<option value={tag.id}>{tag.displayNameJa}</option>{/each}
            </select>
            <button type="button" disabled={busy || !assignTagId} onclick={() => void handleAssignTag()}>付与</button>
          </div>

          <h2>色</h2>
          <div class="colors">
            {#each detail.colors as color (color.rank)}
              <span class="color">
                <span class="swatch" style={`background:${color.hex}`}></span>
                {color.hex} <em>{Math.round(color.ratio * 100)}% {color.source}</em>
              </span>
            {/each}
            {#if detail.colors.length === 0}<span class="muted">色なし</span>{/if}
          </div>
          {#if colorEditing}
            <div class="inline">
              {#each colorDrafts as _hex, index (index)}
                <input bind:value={colorDrafts[index]} placeholder="#RRGGBB" aria-label="色" />
              {/each}
              {#if colorDrafts.length < 3}
                <button type="button" onclick={() => (colorDrafts = [...colorDrafts, ""])}>色を追加</button>
              {/if}
              <button type="button" disabled={busy} onclick={() => void saveColors()}>保存</button>
              <button type="button" disabled={busy} onclick={() => void resetColorsToAuto()}>自動に戻す</button>
              <button type="button" onclick={() => (colorEditing = false)}>取消</button>
            </div>
          {:else}
            <button type="button" onclick={beginColorEdit}>色を修正</button>
          {/if}

          {#if viewMode === "album"}
            <h2>アルバム内の位置</h2>
            <div class="inline">
              <button type="button" disabled={busy} onclick={() => void moveInAlbum(-1)}>↑ 前へ</button>
              <button type="button" disabled={busy} onclick={() => void moveInAlbum(1)}>↓ 後ろへ</button>
              <button type="button" disabled={busy} onclick={() => void removeSelectionFromAlbum()}>アルバムから外す</button>
            </div>
          {/if}

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
  .app { display: grid; grid-template-columns: 320px minmax(0, 1fr); height: 100vh; }
  aside { display: flex; flex-direction: column; gap: 12px; padding: 16px; overflow: auto; border-right: 1px solid #333; background: #1b1b1b; }
  main { display: grid; grid-template-rows: auto auto minmax(0, 1fr); min-width: 0; }
  h1 { margin: 0; font-size: 22px; }
  h2 { margin: 4px 0; font-size: 13px; color: #c9c9c9; }
  .stack { display: grid; gap: 6px; }
  input, select { min-width: 0; padding: 6px; border: 1px solid #555; border-radius: 5px; background: #232323; color: inherit; }
  button { padding: 6px 9px; border: 1px solid #555; border-radius: 5px; background: #2b2b2b; color: inherit; cursor: pointer; }
  button:disabled { cursor: not-allowed; opacity: 0.6; }
  button.active { border-color: #5b9dff; background: #24406b; }
  button.danger { border-color: #a33; background: #4a1d1d; }
  .albums { display: grid; gap: 6px; padding-top: 8px; border-top: 1px solid #333; }
  .album-list { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .album-list li { display: flex; flex-wrap: wrap; align-items: center; gap: 3px; padding: 3px; border: 1px solid transparent; border-radius: 5px; }
  .album-list li.current { border-color: #5b9dff; background: #1d2a3d; }
  .album-open { flex: 1; overflow: hidden; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .roots { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .roots li { display: grid; gap: 3px; padding: 8px; border: 1px solid #3a3a3a; border-radius: 6px; }
  .roots li.disabled { opacity: 0.55; }
  .root-name { overflow: hidden; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .root-meta, .muted { color: #a3a3a3; font-size: 12px; }
  .root-actions, .filter-row, .inline { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; }
  .filter-list { display: grid; max-height: 150px; overflow: auto; gap: 2px; padding: 4px; border: 1px solid #333; border-radius: 5px; }
  .check { display: flex; align-items: center; gap: 5px; font-size: 12px; }
  .num { width: 72px; }
  .tags { display: grid; gap: 7px; padding-top: 8px; border-top: 1px solid #333; }
  .tag-manager { border-top: 1px solid #333; padding-top: 8px; }
  .tag-manager summary { cursor: pointer; font-weight: 600; }
  .tag-admin { display: grid; gap: 8px; padding: 0; list-style: none; }
  .tag-admin > li { display: grid; gap: 4px; padding: 7px; border: 1px solid #333; border-radius: 5px; }
  .tag-title { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; }
  .swatch { width: 12px; height: 12px; border-radius: 50%; }
  .aliases { margin: 3px 0; padding-left: 18px; font-size: 12px; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 10px 14px; border-bottom: 1px solid #333; }
  .stages { display: flex; flex: 1; flex-wrap: wrap; gap: 10px; color: #b6b6b6; font-size: 12px; }
  .error { margin: 0; padding: 8px 14px; background: #4a1d1d; color: #ffb4b4; }
  .content { display: grid; grid-template-columns: minmax(0, 1fr) 340px; min-height: 0; }
  .grid-area { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .grid-area :global(.virtual-grid) { flex: 1; }
  .empty { padding: 24px; }
  .more { margin: 0 10px 10px; }
  .detail { padding: 12px; overflow: auto; border-left: 1px solid #333; background: #181818; }
  .preview { display: block; width: 100%; max-height: 280px; object-fit: contain; background: #0e0e0e; }
  dl { display: grid; grid-template-columns: 70px minmax(0, 1fr); gap: 4px 8px; font-size: 12px; }
  dt { color: #999; }
  dd { margin: 0; overflow-wrap: anywhere; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip { display: inline-flex; align-items: center; gap: 4px; padding: 3px 6px; border: 1px solid #4d6a45; border-radius: 12px; background: #24301f; font-size: 12px; }
  .chip.suggested { border-color: #6a6040; background: #2f2a1d; }
  .chip em { color: #a3a3a3; font-size: 10px; }
  .chip button { padding: 0 4px; border: 0; background: none; }
  .colors { display: flex; flex-wrap: wrap; gap: 8px; font-size: 12px; }
  .color { display: inline-flex; align-items: center; gap: 4px; }
  .color em { color: #999; }
  .locations { display: grid; gap: 6px; padding: 0; list-style: none; font-size: 12px; }
  .locations span { overflow-wrap: anywhere; }
  .missing { color: #e08a8a; }
  @media (max-width: 900px) {
    .app { grid-template-columns: 250px minmax(0, 1fr); }
    .content { grid-template-columns: minmax(0, 1fr); }
    .detail { border-top: 1px solid #333; border-left: 0; }
  }
</style>
