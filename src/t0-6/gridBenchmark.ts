export interface GridBenchmarkResult {
  durationMs: number;
  frameP50Ms: number;
  frameP95Ms: number;
  droppedFrameRate: number;
  frameCount: number;
}

function percentile(values: number[], percentileValue: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(
    sorted.length - 1,
    Math.floor((sorted.length - 1) * percentileValue)
  );
  return sorted[index] ?? 0;
}

function nextFrame(): Promise<number> {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

/** Measures frame scheduling while repeatedly moving a virtual-grid viewport. */
export async function measureScrollPerformance(
  viewport: HTMLDivElement,
  durationMs = 1500
): Promise<GridBenchmarkResult> {
  const frameTimes: number[] = [];
  let lastFrame = await nextFrame();
  const startedAt = performance.now();
  const maxScroll = Math.max(0, viewport.scrollHeight - viewport.clientHeight);

  while (performance.now() - startedAt < durationMs) {
    const now = await nextFrame();
    frameTimes.push(now - lastFrame);
    lastFrame = now;
    const progress = Math.min(1, (now - startedAt) / durationMs);
    viewport.scrollTop = progress * maxScroll;
  }

  const droppedFrames = frameTimes.filter((frameTime) => frameTime > 16.7).length;
  return {
    durationMs: performance.now() - startedAt,
    frameP50Ms: percentile(frameTimes, 0.5),
    frameP95Ms: percentile(frameTimes, 0.95),
    droppedFrameRate: frameTimes.length === 0 ? 0 : droppedFrames / frameTimes.length,
    frameCount: frameTimes.length
  };
}
