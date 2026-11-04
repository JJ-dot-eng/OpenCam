import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { bounds, dragBand, EFFECT_TAGS, effectRows, framesBackMs, framesForMs, prefixMs, range, runs, spanMs, spanText } from "../src/timeline.ts";

const delays = [100, 100, 200, 50, 50, 500];

test("framesForMs finds the frame where the time is reached, or the last frame", () => {
  assert.equal(framesForMs(delays, 0, 100), 0);
  assert.equal(framesForMs(delays, 0, 101), 1);
  assert.equal(framesForMs(delays, 1, 300), 2);
  assert.equal(framesForMs(delays, 2, 10_000), 5);
  assert.equal(framesForMs(delays, 5, 1), 5);
});

test("framesBackMs walks back from the end and stops at the floor", () => {
  assert.equal(framesBackMs(delays, 5, 500), 5);
  assert.equal(framesBackMs(delays, 5, 550), 4);
  assert.equal(framesBackMs(delays, 4, 10_000, 2), 2);
});

test("spanMs adds the frame times inside the span and ignores out-of-range indexes", () => {
  assert.equal(spanMs(delays, 0, 2), 400);
  assert.equal(spanMs(delays, 3, 3), 50);
  assert.equal(spanMs(delays, 4, 99), 550);
});

test("dragBand keeps edges in order and respects the minimum length", () => {
  const orig = { start: 2, end: 4 };
  assert.deepEqual(dragBand("start", orig, 0, 6), { start: 0, end: 4 });
  assert.deepEqual(dragBand("start", orig, 5, 6), { start: 4, end: 4 });
  assert.deepEqual(dragBand("start", orig, 5, 6, 2), { start: 3, end: 4 });
  assert.deepEqual(dragBand("end", orig, 0, 6), { start: 2, end: 2 });
  assert.deepEqual(dragBand("end", orig, 9, 6), { start: 2, end: 5 });
  assert.deepEqual(dragBand("end", orig, 0, 6, 2), { start: 2, end: 3 });
});

test("dragBand moves the whole band without leaving the frame list", () => {
  const orig = { start: 2, end: 4 };
  assert.deepEqual(dragBand("move", orig, 1, 6), { start: 3, end: 5 });
  assert.deepEqual(dragBand("move", orig, 5, 6), { start: 3, end: 5 });
  assert.deepEqual(dragBand("move", orig, -9, 6), { start: 0, end: 2 });
});

test("dragBand never asks for more frames than exist", () => {
  assert.deepEqual(dragBand("end", { start: 0, end: 0 }, 0, 1, 2), { start: 0, end: 0 });
});

test("spanText shows frame numbers from 1 and the length in seconds", () => {
  assert.equal(spanText(delays, { start: 0, end: 2 }), "1~3번 · 0.40초");
  assert.equal(spanText(delays, { start: 5, end: 5 }), "6번 · 0.50초");
});

test("runs groups selected frames into continuous spans, in any order", () => {
  assert.deepEqual(runs([]), []);
  assert.deepEqual(runs([5, 1, 2, 3, 9, 6]), [
    { start: 1, end: 3 },
    { start: 5, end: 6 },
    { start: 9, end: 9 },
  ]);
  assert.deepEqual(runs([4, 4, 5]), [{ start: 4, end: 5 }]);
});

test("bounds finds the first and last frame in one pass, even for huge selections", () => {
  assert.equal(bounds([]), null);
  assert.deepEqual(bounds([7, 2, 9, 4]), { start: 2, end: 9 });
  const huge = Array.from({ length: 300_000 }, (_, i) => i + 5);
  assert.deepEqual(bounds(huge), { start: 5, end: 300_004 });
});

test("effectRows makes one row per used effect with its continuous spans", () => {
  assert.deepEqual(effectRows([0, 0, 0]), []);
  // 필터(1)는 1~2번, 줌(16)은 2번과 4번, 그리기·텍스트·전환은 안 씀
  const rows = effectRows([0, 1, 17, 0, 16]);
  assert.deepEqual(
    rows.map((r) => [r.key, r.spans]),
    [
      ["filter", [{ start: 1, end: 2 }]],
      ["zoom", [{ start: 2, end: 2 }, { start: 4, end: 4 }]],
    ],
  );
});

test("effect tag bits match the Rust frame::tag constants", () => {
  // Rust(frame.rs)와 화면(timeline.ts)이 같은 비트를 쓰는지 맞춰 본다
  const rust = readFileSync(new URL("../src-tauri/src/frame.rs", import.meta.url), "utf8");
  const consts = Object.fromEntries([...rust.matchAll(/pub const (\w+): u8 = (\d+);/g)].map((m) => [m[1].toLowerCase(), Number(m[2])]));
  assert.deepEqual(Object.fromEntries(EFFECT_TAGS.map((t) => [t.key, t.bit])), consts);
});

test("range, prefixMs and runs on already sorted input", () => {
  assert.deepEqual(range(3, 6), [3, 4, 5, 6]);
  assert.deepEqual(range(5, 4), []);
  assert.deepEqual(prefixMs([100, 50, 200]), [0, 100, 150, 350]);
  assert.deepEqual(runs([1, 2, 3, 7, 8]), [{ start: 1, end: 3 }, { start: 7, end: 8 }]);
});
