import test from "node:test";
import assert from "node:assert/strict";
import { dragBand, framesBackMs, framesForMs, spanMs, spanText } from "../src/timeline.ts";

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
