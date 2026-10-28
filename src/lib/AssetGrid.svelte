<script lang="ts">
  import { onMount } from "svelte";
  import { visibleRange } from "../t0-6/gridMath";
  import type { AssetSummaryView } from "./generated/ui-contract";

  export let items: AssetSummaryView[] = [];
  export let selectedIds = new Set<string>();
  export let onSelect: (asset: AssetSummaryView) => void = () => {};
  export let onContextMenu: (asset: AssetSummaryView) => void = () => {};
  export let onNearEnd: () => void = () => {};
  export let onVisible: (assetIds: string[]) => void = () => {};
  export let tileWidth = 176;
  export let tileHeight = 176;
  export let overscanRows = 3;
  export let thumbnail: (assetId: string) => string = () => "";

  let viewport: HTMLDivElement;
  let viewportWidth = 900;
  let viewportHeight = 600;
  let scrollTop = 0;
  let lastVisible = "";

  $: range = visibleRange(items.length, viewportWidth, viewportHeight, scrollTop, tileWidth, tileHeight, overscanRows);
  $: columns = range.columns;
  $: rowCount = range.rowCount;
  $: visibleStart = range.start;
  $: visibleItems = items.slice(visibleStart, range.end);
  $: {
    const ids = visibleItems.map((asset) => asset.id).join(",");
    if (ids !== lastVisible) {
      lastVisible = ids;
      onVisible(visibleItems.map((asset) => asset.id));
    }
  }

  function measureViewport() {
    if (!viewport) return;
    viewportWidth = viewport.clientWidth;
    viewportHeight = viewport.clientHeight;
  }

  function handleScroll(event: Event) {
    const element = event.currentTarget;
    if (element instanceof HTMLDivElement) {
      scrollTop = element.scrollTop;
      if (scrollTop + viewportHeight >= rowCount * tileHeight * 0.8) onNearEnd();
    }
  }

  onMount(() => {
    const observer = new ResizeObserver(measureViewport);
    observer.observe(viewport);
    measureViewport();
    return () => observer.disconnect();
  });
</script>

<div class="virtual-grid" bind:this={viewport} onscroll={handleScroll}>
  <div class="grid-spacer" style={`height: ${rowCount * tileHeight}px`}>
    {#each visibleItems as asset, index (asset.id)}
      {@const absoluteIndex = visibleStart + index}
      {@const column = absoluteIndex % columns}
      {@const row = Math.floor(absoluteIndex / columns)}
      <button
        class="grid-tile"
        class:selected={selectedIds.has(asset.id)}
        style={`left: ${column * tileWidth}px; top: ${row * tileHeight}px; width: ${tileWidth}px; height: ${tileHeight}px`}
        type="button"
        aria-label={`画像 ${asset.id}`}
        onclick={() => onSelect(asset)}
        oncontextmenu={(event) => {
          event.preventDefault();
          onContextMenu(asset);
        }}
      >
        {#if asset.thumbnailState === "ready"}
          <img src={thumbnail(asset.id)} alt="" loading="lazy" />
        {:else}
          <span class="pending">処理待ち</span>
        {/if}
        {#if asset.locationStatus !== "present"}
          <span class="badge">{asset.locationStatus}</span>
        {/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .virtual-grid { min-height: 0; overflow: auto; contain: strict; background: #151515; }
  .grid-spacer { position: relative; width: 100%; }
  .grid-tile {
    position: absolute;
    box-sizing: border-box;
    padding: 3px;
    overflow: hidden;
    border: 2px solid transparent;
    border-radius: 6px;
    background: #242424;
    cursor: pointer;
  }
  .grid-tile:hover, .grid-tile:focus-visible { border-color: #78a9ff; outline: none; }
  .grid-tile.selected { border-color: #5b9dff; }
  .grid-tile img { display: block; width: 100%; height: 100%; border-radius: 3px; object-fit: cover; background: #303030; }
  .pending { color: #888; font-size: 12px; }
  .badge {
    position: absolute;
    right: 6px;
    bottom: 6px;
    padding: 2px 5px;
    border-radius: 3px;
    background: #000a;
    color: #fff;
    font-size: 10px;
  }
</style>
