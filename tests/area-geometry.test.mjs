import test from "node:test";
import assert from "node:assert/strict";
import { selectionRect } from "../src/areaGeometry.ts";

test("selection uses physical pixels, including negative desktop origins and reverse drags", () => {
  const viewport = { width: 2000, height: 1000 };
  const physical = { width: 3000, height: 1250 };
  const origin = { x: -1920, y: -200 };
  const a = { x: 1500, y: 800 }, b = { x: 200, y: 100 };
  const expected = { x: -1620, y: -75, w: 1950, h: 875 };
  assert.deepEqual(selectionRect(a, b, viewport, origin, physical), expected);
  assert.deepEqual(selectionRect(b, a, viewport, origin, physical), expected);
});

test("selection rejects a zero viewport or physical window size", () => {
  const p = { x: 1, y: 2 }, size = { width: 100, height: 100 };
  assert.throws(() => selectionRect(p, p, { ...size, width: 0 }, p, size));
  assert.throws(() => selectionRect(p, p, size, p, { ...size, height: 0 }));
});
