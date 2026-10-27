import assert from "node:assert/strict";
import test from "node:test";
import { summarizeFrameTimes, percentile } from "./gridBenchmark.ts";
import { visibleRange } from "./gridMath.ts";

test("visibleRange keeps a 10,000 item grid virtualized", () => {
  const range = visibleRange(10_000, 900, 600, 100_000, 176, 176, 3);
  assert.equal(range.columns, 5);
  assert.ok(range.end - range.start < 100);
  assert.ok(range.start > 0);
  assert.ok(range.end < 10_000);
});

test("visibleRange clamps at both ends", () => {
  const first = visibleRange(10, 900, 600, -100, 176, 176, 3);
  const last = visibleRange(10, 900, 600, 100_000, 176, 176, 3);
  assert.equal(first.start, 0);
  assert.equal(last.end, 10);
});

test("percentile uses the nearest-rank definition", () => {
  assert.equal(percentile([4, 1, 3, 2], 0.5), 2);
  assert.equal(percentile([4, 1, 3, 2], 0.95), 4);
  assert.equal(percentile([], 0.95), 0);
});

test("frame summary reports fps, p95 and over-budget frames", () => {
  const result = summarizeFrameTimes([10, 10, 20, 20]);
  assert.equal(result.averageFps, 200 / 3);
  assert.equal(result.frameP95Ms, 20);
  assert.equal(result.droppedFrameRate, 0.5);
});
