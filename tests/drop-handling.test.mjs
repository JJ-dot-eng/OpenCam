import test from "node:test";
import assert from "node:assert/strict";
import { classifyOpenPaths, extOf, getDropKind, handleFileDrop } from "../src/dropHandling.ts";

const rejectedToast = "처리 중이거나 창·도구가 열려 있어 파일을 받지 않았습니다";
const idleState = () => ({
  appending: false, busy: false, dialog: null, importReq: null, menu: null, mode: null,
  isModalOpen: () => false, frameCount: 1,
});

function dropHarness(state = idleState()) {
  const events = [];
  const handlers = {
    kind: () => { events.push(["검사"]); return getDropKind(state); },
    setHint: (kind) => events.push(["힌트", kind]),
    run: (paths) => events.push(["실행", paths]),
    showToast: (message) => events.push(["알림", message]),
  };
  return { state, events, send: (event) => handleFileDrop(event, handlers) };
}

test("프레임이 있으면 붙이고 없으면 연다", () => {
  assert.equal(getDropKind(idleState()), "append");
  assert.equal(getDropKind({ ...idleState(), frameCount: 0 }), "open");
});

const blockers = [
  ["붙이기", { appending: true }],
  ["처리", { busy: true }],
  ["대화상자", { dialog: "settings" }],
  ["가져오기 창", { importReq: { kind: "images", paths: ["a.jpg"] } }],
  ["오른쪽 클릭 메뉴", { menu: { x: 0, y: 0 } }],
  ["도구", { mode: "crop" }],
  ["확인 창", { isModalOpen: () => true }],
];
for (const [name, blocked] of blockers) {
  test(`${name} 중에는 프레임 수와 무관하게 드롭을 거절하고 같은 문구를 알린다`, () => {
    for (const frameCount of [0, 2]) {
      const h = dropHarness({ ...idleState(), ...blocked, frameCount });
      assert.equal(getDropKind(h.state), null);
      h.send({ type: "enter", paths: ["a.png"] });
      h.send({ type: "over" });
      h.send({ type: "drop", paths: ["a.png"] });
      assert.deepEqual(h.events, [
        ["검사"], ["힌트", null], ["검사"], ["힌트", null],
        ["힌트", null], ["검사"], ["알림", rejectedToast],
      ]);
    }
  });
}

test("앞 조건에 막히면 확인 창 상태는 검사하지 않는다", () => {
  for (const [, blocked] of blockers.slice(0, -1)) {
    assert.equal(getDropKind({
      ...idleState(), ...blocked,
      isModalOpen: () => assert.fail("앞 조건이 먼저 막아야 합니다"),
    }), null);
  }
});

test("들어옴·이동에는 지금 상태의 힌트를 보이고 떠남에는 검사 없이 지운다", () => {
  const h = dropHarness();
  h.send({ type: "enter", paths: ["a.png"] });
  h.state.frameCount = 0;
  h.send({ type: "over" });
  h.send({ type: "leave" });
  assert.deepEqual(h.events, [
    ["검사"], ["힌트", "append"], ["검사"], ["힌트", "open"], ["힌트", null],
  ]);
});

test("경로가 없는 드롭은 힌트만 지우고 검사·실행·거절 알림을 하지 않는다", () => {
  for (const appending of [false, true]) {
    const h = dropHarness({ ...idleState(), appending });
    h.send({ type: "drop", paths: [] });
    assert.deepEqual(h.events, [["힌트", null]]);
  }
});

test("받는 드롭도 힌트를 먼저 지우고 원래 경로 목록을 전달한다", () => {
  for (const frameCount of [0, 2]) {
    const paths = ["C:\\사진\\b.PNG", "/사진/a.JPG"];
    const h = dropHarness({ ...idleState(), frameCount });
    h.send({ type: "drop", paths });
    assert.deepEqual(h.events, [["힌트", null], ["검사"], ["실행", paths]]);
    assert.equal(h.events[2][1], paths);
  }
});

test("들어올 때 허용했어도 놓을 때 바빠졌으면 거절한다", () => {
  const h = dropHarness();
  h.send({ type: "enter", paths: ["a.png"] });
  h.state.busy = true;
  h.send({ type: "drop", paths: ["a.png"] });
  assert.deepEqual(h.events.slice(2), [["힌트", null], ["검사"], ["알림", rejectedToast]]);
});

test("들어올 때 막혔어도 놓을 때 풀렸으면 받는다", () => {
  const h = dropHarness({ ...idleState(), busy: true });
  h.send({ type: "enter", paths: ["a.png"] });
  h.state.busy = false;
  h.send({ type: "drop", paths: ["a.png"] });
  assert.deepEqual(h.events.slice(2), [["힌트", null], ["검사"], ["실행", ["a.png"]]]);
});

test("지원하는 동영상 확장자는 대문자와 두 경로 구분자에서도 구간 가져오기로 보낸다", () => {
  for (const ext of ["mp4", "webm", "mkv", "mov", "avi", "wmv", "m4v", "flv", "mpg", "mpeg", "ts"]) {
    for (const dir of ["C:\\영상\\", "/home/영상/"]) {
      for (const suffix of [ext, ext.toUpperCase()]) {
        const path = `${dir}clip.${suffix}`;
        assert.deepEqual(classifyOpenPaths([path]), { kind: "video", path });
      }
    }
  }
});

test("동영상 두 개와 동영상·이미지 혼합은 모두 하나씩 열라는 알림으로 거절한다", () => {
  for (const paths of [
    ["C:\\a.MP4", "/b.webm"], ["a.jpg", "b.MOV"], ["a.mp4", "b.png"],
  ]) {
    assert.deepEqual(classifyOpenPaths(paths), { kind: "reject", message: "동영상은 하나씩 따로 열어 주세요" });
  }
});

test("GIF·WebP·PNG·APNG 하나는 대문자와 두 경로 구분자에서도 바로 불러온다", () => {
  for (const ext of ["gif", "webp", "png", "apng"]) {
    for (const dir of ["C:\\그림\\", "/home/그림/"]) {
      for (const suffix of [ext, ext.toUpperCase()]) {
        const path = `${dir}그림.${suffix}`;
        assert.deepEqual(classifyOpenPaths([path]), { kind: "import", paths: [path] });
      }
    }
  }
});

test("다른 그림 하나나 여러 그림은 이미지 옵션으로 보내고 원래 목록을 바꾸지 않는다", () => {
  for (const path of ["C:\\그림\\a.JPG", "/home/a.bmp", "a.tiff", "a.avif", "확장자없음", "a.unknown"]) {
    assert.deepEqual(classifyOpenPaths([path]), { kind: "images", paths: [path] });
  }
  const paths = Object.freeze(["b.gif", "a.PNG"]);
  assert.deepEqual(classifyOpenPaths(paths), { kind: "images", paths: ["a.PNG", "b.gif"] });
  assert.deepEqual(paths, ["b.gif", "a.PNG"]);
});

test("폴더의 점이나 파일 중간의 확장자는 동영상 확장자로 세지 않는다", () => {
  for (const path of ["C:\\folder.MP4\\image", "/folder.mov/image", "clip.MP4.jpg", "clip.", "clip"]) {
    assert.notEqual(classifyOpenPaths([path]).kind, "video");
  }
  assert.equal(extOf("C:\\folder.MOV\\clip.MP4"), "mp4");
  assert.equal(extOf("/folder.mp4/clip"), "");
  assert.equal(extOf("clip."), "");
});
