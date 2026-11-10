export type ScrollMode = "continuous" | "rapid";

export interface GridBenchmarkResult {
  mode: string;
  durationMs: number;
  averageFps: number;
  frameP50Ms: number;
  frameP95Ms: number;
  droppedFrameRate: number;
  frameCount: number;
  domNodeCount: number;
  renderedTileCount: number;
  jsHeapBytes?: number;
}

interface PerformanceWithMemory extends Performance {
  memory?: { usedJSHeapSize: number };
}

export function percentile(values: number[], percentileValue: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  if (sorted.length === 0) return 0;
  const index = Math.min(
    sorted.length - 1,
    Math.ceil(sorted.length * percentileValue) - 1
  );
  return sorted[index] ?? 0;
}

function nextFrame(): Promise<number> {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

export function summarizeFrameTimes(frameTimes: number[]) {
  const total = frameTimes.reduce((sum, value) => sum + value, 0);
  const droppedFrames = frameTimes.filter((frameTime) => frameTime > 16.7).length;
  return {
    averageFps: total === 0 ? 0 : (frameTimes.length * 1000) / total,
    frameP50Ms: percentile(frameTimes, 0.5),
    frameP95Ms: percentile(frameTimes, 0.95),
    droppedFrameRate: frameTimes.length === 0 ? 0 : droppedFrames / frameTimes.length,
    frameCount: frameTimes.length
  };
}

/** Measures frame scheduling while moving a virtual-grid viewport without blocking the UI thread. */
export async function measureScrollPerformance(
  viewport: HTMLDivElement,
  mode: string,
  durationMs = 2500
): Promise<GridBenchmarkResult> {
  viewport.scrollTop = 0;
  await nextFrame();
  const frameTimes: number[] = [];
  let lastFrame = await nextFrame();
  const startedAt = performance.now();
  const maxScroll = Math.max(0, viewport.scrollHeight - viewport.clientHeight);

  while (performance.now() - startedAt < durationMs) {
    const now = await nextFrame();
    frameTimes.push(now - lastFrame);
    lastFrame = now;
    const progress = Math.min(1, (now - startedAt) / durationMs);
    viewport.scrollTop =
      mode === "continuous"
        ? progress * maxScroll
        : (Math.floor(progress * 24) % 2 === 0 ? progress : 1 - progress) * maxScroll;
  }

  const memory = performance as PerformanceWithMemory;
  return {
    mode,
    durationMs: performance.now() - startedAt,
    ...summarizeFrameTimes(frameTimes),
    domNodeCount: document.getElementsByTagName("*").length,
    renderedTileCount: document.querySelectorAll(".grid-tile").length,
    jsHeapBytes: memory.memory?.usedJSHeapSize
  };
}
