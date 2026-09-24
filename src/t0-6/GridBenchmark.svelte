<script lang="ts">
  import { tick } from "svelte";
  import VirtualGrid, { type GridItem } from "./VirtualGrid.svelte";
  import { measureScrollPerformance, type GridBenchmarkResult } from "./gridBenchmark";

  const items: GridItem[] = Array.from({ length: 10_000 }, (_, index) => ({
    assetId: `benchmark-${index.toString().padStart(5, "0")}`,
    label: `Asset ${index + 1}`
  }));

  let viewport: HTMLDivElement | undefined;
  let result: GridBenchmarkResult | undefined;
  let selectedAsset = "";
  let running = false;

  async function runBenchmark() {
    if (!viewport || running) return;
    running = true;
    result = undefined;
    await tick();
    result = await measureScrollPerformance(viewport);
    running = false;
  }
</script>

<section class="benchmark">
  <div class="toolbar">
    <div>
      <h2>均等グリッド検証</h2>
      <p>10,000件 / asset IDのみ / masonryなし</p>
    </div>
    <button type="button" onclick={runBenchmark} disabled={running}>
      {running ? "計測中…" : "スクロール計測"}
    </button>
  </div>

  <VirtualGrid
    {items}
    onSelect={(assetId) => (selectedAsset = assetId)}
    onViewportReady={(element) => (viewport = element)}
  />

  {#if selectedAsset}
    <p class="status">選択: {selectedAsset}</p>
  {/if}

  {#if result}
    <dl class="result">
      <div><dt>フレーム p50</dt><dd>{result.frameP50Ms.toFixed(2)} ms</dd></div>
      <div><dt>フレーム p95</dt><dd>{result.frameP95Ms.toFixed(2)} ms</dd></div>
      <div><dt>16.7ms超過率</dt><dd>{(result.droppedFrameRate * 100).toFixed(1)}%</dd></div>
      <div><dt>フレーム数</dt><dd>{result.frameCount}</dd></div>
    </dl>
  {/if}
</section>

<style>
  .benchmark {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    min-height: calc(100vh - 96px);
    gap: 16px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    font-size: 20px;
  }

  .toolbar p,
  .status {
    color: #a0a0a0;
    font-size: 13px;
  }

  button {
    padding: 8px 12px;
    border: 1px solid #555;
    border-radius: 5px;
    color: #f5f5f5;
    background: #2c2c2c;
    cursor: pointer;
  }

  button:disabled {
    cursor: wait;
    opacity: 0.6;
  }

  .result {
    display: flex;
    gap: 24px;
    margin: 0;
    color: #cfcfcf;
    font-size: 13px;
  }

  .result div {
    display: flex;
    gap: 6px;
  }

  dt {
    color: #8e8e8e;
  }

  dd {
    margin: 0;
  }
</style>
