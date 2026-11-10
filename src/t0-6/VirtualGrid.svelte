<script lang="ts" context="module">
  export interface GridItem {
    assetId: string;
    label: string;
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { visibleRange } from "./gridMath";

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

  $: range = visibleRange(items.length, viewportWidth, viewportHeight, scrollTop, tileWidth, tileHeight, overscanRows);
  $: columns = range.columns;
  $: rowCount = range.rowCount;
  $: visibleStart = range.start;
  $: visibleEnd = range.end;
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
    return `asset://localhost/thumbnail/${encodeURIComponent(assetId)}?size=grid`;
  }

  onMount(() => {
    const observer = new ResizeObserver(measureViewport);
    observer.observe(viewport);
    measureViewport();
    onViewportReady(viewport);
    return () => observer.disconnect();
  });
</script>

<div class="virtual-grid" bind:this={viewport} onscroll={handleScroll}>
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
