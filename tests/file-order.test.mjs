import test from "node:test";
import assert from "node:assert/strict";
import { baseName, compareFileNames, sortByFileName } from "../src/fileOrder.ts";

test("numbers in file names compare by value", () => {
  assert.ok(compareFileNames("C:\\v\\clip2.mp4", "C:\\v\\clip10.mp4") < 0);
  assert.deepEqual(sortByFileName(["clip10.gif", "clip1.gif", "clip2.gif"]), ["clip1.gif", "clip2.gif", "clip10.gif"]);
});

test("only the file name decides the order, not the folder", () => {
  assert.deepEqual(sortByFileName(["C:\\a\\b2.gif", "C:\\z\\a1.gif"]), ["C:\\z\\a1.gif", "C:\\a\\b2.gif"]);
  assert.deepEqual(sortByFileName(["/a/b2.gif", "/z/a1.gif"]), ["/z/a1.gif", "/a/b2.gif"]);
});

test("case does not matter", () => {
  assert.deepEqual(sortByFileName(["clip10.mp4", "Clip3.mp4"]), ["Clip3.mp4", "clip10.mp4"]);
  assert.deepEqual(sortByFileName(["B.png", "a.png", "C.png"]), ["a.png", "B.png", "C.png"]);
});

test("windows and posix separators both work", () => {
  assert.equal(baseName("C:\\Users\\me\\Videos\\clip.mp4"), "clip.mp4");
  assert.equal(baseName("/home/me/clip.mp4"), "clip.mp4");
  assert.equal(baseName("C:\\mixed/dir\\clip.mp4"), "clip.mp4");
  assert.equal(baseName("clip.mp4"), "clip.mp4");
  assert.deepEqual(sortByFileName(["C:\\x\\clip10.gif", "/y/clip9.gif"]), ["/y/clip9.gif", "C:\\x\\clip10.gif"]);
});

test("same name in different folders keeps a fixed order", () => {
  const a = sortByFileName(["D:\\b\\same.gif", "D:\\a\\same.gif"]);
  const b = sortByFileName(["D:\\a\\same.gif", "D:\\b\\same.gif"]);
  assert.deepEqual(a, b);
  assert.deepEqual(a, ["D:\\a\\same.gif", "D:\\b\\same.gif"]);
});
