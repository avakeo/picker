<script lang="ts">
  import { tick } from "svelte";
  import VirtualGrid, { type GridItem } from "./VirtualGrid.svelte";
  import { measureScrollPerformance, type GridBenchmarkResult } from "./gridBenchmark";

  const repetitions = 5;
  const initializedAt = performance.now();
  const items: GridItem[] = Array.from({ length: 10_000 }, (_, index) => ({
    assetId: `benchmark-${index.toString().padStart(5, "0")}`,
    label: `Asset ${index + 1}`
  }));
  let viewport: HTMLDivElement | undefined;
  let results: GridBenchmarkResult[] = [];
  let initialRenderMs: number | undefined;
  let selectedAsset = "";
  let running = false;

  async function prepareViewport(element: HTMLDivElement) {
    viewport = element;
    await tick();
    requestAnimationFrame(() => requestAnimationFrame(() => {
      initialRenderMs = performance.now() - initializedAt;
    }));
  }

  async function runBenchmark() {
    if (!viewport || running) return;
    running = true;
    results = [];
    await tick();
    const measured: GridBenchmarkResult[] = [];
    for (const mode of ["continuous", "rapid"]) {
      for (let run = 0; run < repetitions; run += 1) {
        measured.push(await measureScrollPerformance(viewport, mode));
      }
    }
    results = measured;
    running = false;
  }
</script>

<section class="benchmark">
  <div class="toolbar">
    <div><h2>均等グリッド検証</h2><p>10,000件 / asset ID＋sizeのみ / masonryなし</p></div>
    <button type="button" onclick={runBenchmark} disabled={running} data-benchmark-start>
      {running ? "計測中…" : "全計測を開始"}
    </button>
  </div>
  <VirtualGrid {items} onSelect={(assetId) => (selectedAsset = assetId)} onViewportReady={prepareViewport} />
  {#if selectedAsset}<p class="status">選択: {selectedAsset}</p>{/if}
  <div class="summary" data-benchmark-summary>
    <span>初期表示: {initialRenderMs === undefined ? "計測待ち" : `${initialRenderMs.toFixed(2)} ms`}</span>
    <span>DOM: {typeof document === "undefined" ? 0 : document.getElementsByTagName("*").length}</span>
    <span>表示タイル: {typeof document === "undefined" ? 0 : document.querySelectorAll(".grid-tile").length}</span>
  </div>
  {#if results.length > 0}
    <table>
      <thead><tr><th>方式</th><th>回</th><th>FPS</th><th>p50</th><th>p95</th><th>&gt;16.7ms</th><th>DOM</th><th>タイル</th></tr></thead>
      <tbody>{#each results as result, index (result.mode + index)}<tr>
        <td>{result.mode}</td><td>{(index % repetitions) + 1}</td><td>{result.averageFps.toFixed(2)}</td>
        <td>{result.frameP50Ms.toFixed(2)}</td><td>{result.frameP95Ms.toFixed(2)}</td>
        <td>{(result.droppedFrameRate * 100).toFixed(1)}%</td><td>{result.domNodeCount}</td><td>{result.renderedTileCount}</td>
      </tr>{/each}</tbody>
    </table>
    <pre data-benchmark-json>{JSON.stringify({ viewportWidth: window.innerWidth, initialRenderMs, results })}</pre>
  {/if}
</section>

<style>
  .benchmark { display: grid; grid-template-rows: auto minmax(0, 1fr) auto auto; min-height: calc(100vh - 96px); gap: 12px; }
  .toolbar, .summary { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  h2, p { margin: 0; }
  h2 { font-size: 20px; }
  .toolbar p, .status, .summary { color: #a0a0a0; font-size: 13px; }
  button { padding: 8px 12px; border: 1px solid #555; border-radius: 5px; color: #f5f5f5; background: #2c2c2c; cursor: pointer; }
  button:disabled { cursor: wait; opacity: 0.6; }
  table { border-collapse: collapse; color: #cfcfcf; font-size: 12px; }
  th, td { padding: 3px 8px; border: 1px solid #444; text-align: right; }
  th:first-child, td:first-child { text-align: left; }
  pre { max-height: 80px; overflow: auto; font-size: 10px; white-space: pre-wrap; }
</style>
