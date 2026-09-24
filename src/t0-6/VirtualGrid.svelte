<script lang="ts">
  import { onMount } from "svelte";

  export interface GridItem {
    assetId: string;
    label: string;
  }

  export let items: GridItem[] = [];
  export let tileWidth = 176;
  export let tileHeight = 176;
  export let overscanRows = 3;
  export let onSelect: (assetId: string) => void = () => {};
  export let onViewportReady: (element: HTMLDivElement) => void = () => {};

  let viewport: HTMLDivElement;
  let viewportWidth = 900;
  let viewportHeight = 600;
  let scrollTop = 0;

  $: columns = Math.max(1, Math.floor(viewportWidth / tileWidth));
  $: rowCount = Math.ceil(items.length / columns);
  $: firstRow = Math.max(0, Math.floor(scrollTop / tileHeight) - overscanRows);
  $: lastRow = Math.min(
    rowCount,
    Math.ceil((scrollTop + viewportHeight) / tileHeight) + overscanRows
  );
  $: visibleStart = firstRow * columns;
  $: visibleEnd = Math.min(items.length, lastRow * columns);
  $: visibleItems = items.slice(visibleStart, visibleEnd);

  function measureViewport() {
    if (!viewport) return;
    viewportWidth = viewport.clientWidth;
    viewportHeight = viewport.clientHeight;
  }

  function handleScroll(event: Event) {
    const element = event.currentTarget;
    if (element instanceof HTMLDivElement) {
      scrollTop = element.scrollTop;
    }
  }

  function thumbnailUrl(assetId: string) {
    return `asset://thumbnail/${encodeURIComponent(assetId)}?size=medium`;
  }

  onMount(() => {
    const observer = new ResizeObserver(measureViewport);
    observer.observe(viewport);
    measureViewport();
    onViewportReady(viewport);
    return () => observer.disconnect();
  });
</script>

<div class="virtual-grid" bind:this={viewport} on:scroll={handleScroll}>
  <div class="grid-spacer" style={`height: ${rowCount * tileHeight}px`}>
    {#each visibleItems as item, index (item.assetId)}
      {@const absoluteIndex = visibleStart + index}
      {@const column = absoluteIndex % columns}
      {@const row = Math.floor(absoluteIndex / columns)}
      <button
        class="grid-tile"
        style={`left: ${column * tileWidth}px; top: ${row * tileHeight}px; width: ${tileWidth}px; height: ${tileHeight}px`}
        type="button"
        aria-label={item.label}
        onclick={() => onSelect(item.assetId)}
      >
        <img src={thumbnailUrl(item.assetId)} alt="" loading="lazy" />
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .virtual-grid {
    min-height: 0;
    overflow: auto;
    contain: strict;
    background: #151515;
  }

  .grid-spacer {
    position: relative;
    width: 100%;
  }

  .grid-tile {
    position: absolute;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    border: 1px solid #333;
    border-radius: 6px;
    color: #f5f5f5;
    background: #242424;
    text-align: left;
    cursor: pointer;
  }

  .grid-tile:hover,
  .grid-tile:focus-visible {
    border-color: #78a9ff;
    outline: none;
  }

  .grid-tile img {
    display: block;
    width: 100%;
    min-height: 0;
    flex: 1;
    object-fit: cover;
    border-radius: 4px;
    background: #303030;
  }

  .grid-tile span {
    overflow: hidden;
    color: #cfcfcf;
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
