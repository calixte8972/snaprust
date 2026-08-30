import assert from "node:assert/strict";
import test from "node:test";

import { createAsyncPool } from "../src/async-pool.ts";
import { mosaicRgbaPixels } from "../src/image-processing.ts";
import { normalizeSelection, selectionToPhysicalPixels, simplifyPolyline } from "../src/selection.ts";

test("selection normalization and physical projection preserve negative desktop origins", () => {
  const selection = normalizeSelection({ x: 300, y: 250 }, { x: 100, y: 50 }, 800, 400);
  assert.deepEqual(selectionToPhysicalPixels(selection, 1600, 800, -1600, 0), {
    x: -1400,
    y: 100,
    width: 400,
    height: 400,
  });
});

test("polyline simplification retains endpoints", () => {
  const points = Array.from({ length: 101 }, (_, index) => ({ x: index, y: index * 0.01 }));
  const simplified = simplifyPolyline(points, 1);
  assert.deepEqual(simplified, [points[0], points.at(-1)]);
});

test("mosaic uses deterministic block averages", () => {
  const pixels = new Uint8ClampedArray([
    0, 0, 0, 255, 100, 0, 0, 255,
    0, 100, 0, 255, 100, 100, 0, 255,
  ]);
  mosaicRgbaPixels(pixels, 2, 2, 2);
  assert.deepEqual([...pixels], [
    50, 50, 0, 255, 50, 50, 0, 255,
    50, 50, 0, 255, 50, 50, 0, 255,
  ]);
});

test("async pool never exceeds its concurrency limit", async () => {
  const run = createAsyncPool(3);
  let active = 0;
  let peak = 0;
  await Promise.all(Array.from({ length: 12 }, (_, index) => run(async () => {
    active += 1;
    peak = Math.max(peak, active);
    await new Promise((resolve) => setTimeout(resolve, 2));
    active -= 1;
    return index;
  })));
  assert.equal(peak, 3);
});
