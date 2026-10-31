import test from "node:test";
import assert from "node:assert/strict";
import { blocksProjectChange, createAppendGuard, invalidateAppend } from "../src/appendGuard.ts";
import { appendBlockMessage, appendShortcutAction, isProjectDialog, runAppend } from "../src/appendLock.ts";
import { getDropKind, handleFileDrop } from "../src/dropHandling.ts";

const appendToast = "파일을 붙이는 중입니다. 끝난 뒤 다시 해 주세요";
const dropToast = "처리 중이거나 창·도구가 열려 있어 파일을 받지 않았습니다";
const key = (value, ctrlKey = false, metaKey = false) => ({ key: value, ctrlKey, metaKey });

function harness() {
  const pending = Promise.withResolvers();
  const guard = { current: createAppendGuard() };
  const calls = [], views = [], staleViews = [], errors = [], toasts = [], alerts = [], busy = [], events = [], jobs = [];
  const handlers = {
    guard,
    blockWhileAppending: () => toasts.push(appendBlockMessage(guard.current)),
    setBusy: (value) => { busy.push(value); events.push(["처리", value, blocksProjectChange(guard.current)]); },
    setPlaying: (value) => events.push(["재생", value]),
    setBusyText: (value) => events.push(["처리 문구", value]),
    appendFiles: (paths) => { calls.push(paths); events.push(["읽기", blocksProjectChange(guard.current)]); return pending.promise; },
    projectView: async () => { assert.fail("옛 결과일 때만 다시 읽어야 합니다"); },
    applyStaleView: (view) => staleViews.push(view),
    applyView: (view) => { events.push(["결과", blocksProjectChange(guard.current)]); views.push(view); },
    alert: async (message) => alerts.push(message),
    showError: (error) => errors.push(error),
  };
  const start = (paths = ["a.png"]) => runAppend(paths, handlers);
  const drop = (paths = ["a.png"]) => handleFileDrop({ type: "drop", paths }, {
    // React가 다음 화면을 그리기 전처럼 busy는 아직 false다. ref 잠금으로 막아야 한다.
    kind: () => getDropKind({
      appending: blocksProjectChange(guard.current), busy: false,
      dialog: null, importReq: null, menu: null, mode: null,
      isModalOpen: () => false, frameCount: 1,
    }),
    setHint: () => {},
    run: (paths) => jobs.push(start(paths)),
    showToast: (message) => toasts.push(message),
  });
  return { pending, guard, calls, views, staleViews, errors, toasts, alerts, busy, events, jobs, handlers, start, drop };
}

test("같은 순간 두 번 놓아도 첫 await 전에 잠겨 첫 붙이기만 시작한다", async () => {
  const h = harness();
  h.drop(["b.png", "a.png"]);
  h.drop(["c.png"]);
  assert.deepEqual(h.calls, [["a.png", "b.png"]]);
  assert.equal(h.jobs.length, 1);
  assert.deepEqual(h.toasts, [dropToast]);
  assert.deepEqual(h.events, [["처리", true, true], ["재생", false], ["읽기", true]]);
  h.pending.resolve({ view: "붙인 결과", skipped: [] });
  await Promise.all(h.jobs);
  assert.deepEqual(h.views, ["붙인 결과"]);
  assert.deepEqual(h.events.slice(3), [["결과", true], ["처리", false, false], ["처리 문구", "처리 중…"]]);
  assert.equal(appendBlockMessage(h.guard.current), null);
  h.drop(["다음.png"]);
  await Promise.all(h.jobs);
  assert.equal(h.calls.length, 2);
});

test("드롭 검사를 거치지 않고 붙이기를 겹쳐 불러도 두 번째 읽기는 시작하지 않는다", async () => {
  const h = harness();
  const first = h.start();
  await h.start(["b.png"]);
  assert.equal(h.calls.length, 1);
  assert.deepEqual(h.busy, [true]);
  assert.deepEqual(h.toasts, [appendToast]);
  h.pending.resolve({ view: "결과", skipped: [] });
  await first;
});

test("파일 읽기를 기다리는 동안 들어온 드롭도 거절한다", async () => {
  const h = harness();
  h.drop();
  await Promise.resolve();
  h.drop(["b.png"]);
  assert.equal(h.calls.length, 1);
  assert.deepEqual(h.toasts, [dropToast]);
  h.pending.resolve({ view: "결과", skipped: [] });
  await Promise.all(h.jobs);
});

for (const outcome of ["성공", "읽기 실패", "결과 처리 예외", "읽기 시작 예외"]) {
  test(`${outcome} 뒤에는 실제 붙이기 흐름에서 잠금과 처리 표시를 푼다`, async () => {
    const h = harness();
    const error = new Error(outcome);
    if (outcome === "결과 처리 예외") h.handlers.applyView = () => { throw error; };
    if (outcome === "읽기 시작 예외") h.handlers.appendFiles = () => { throw error; };
    const job = h.start();
    if (outcome === "읽기 실패") h.pending.reject(error);
    else h.pending.resolve({ view: "결과", skipped: [] });
    await job;
    assert.equal(blocksProjectChange(h.guard.current), false);
    assert.equal(appendBlockMessage(h.guard.current), null);
    assert.deepEqual(h.busy, [true, false]);
    assert.deepEqual(h.events.at(-1), ["처리 문구", "처리 중…"]);
    assert.deepEqual(h.errors, outcome === "성공" ? [] : [error]);
    assert.deepEqual(h.views, outcome === "성공" ? ["결과"] : []);
    assert.equal(appendShortcutAction(h.guard.current, key("o", true)), "allow");
  });
}

test("건너뛴 파일 안내가 닫힐 때까지 잠금을 유지한다", async () => {
  const h = harness();
  const notice = Promise.withResolvers();
  h.handlers.alert = (message) => { h.alerts.push(message); return notice.promise; };
  const job = h.start(["a.png", "b.png"]);
  h.pending.resolve({ view: "일부 결과", skipped: [{ name: "a.png", reason: "읽기 실패" }] });
  await Promise.resolve();
  assert.deepEqual(h.alerts, ["1개 파일은 붙이지 못하고 건너뛰었습니다.\n\n· a.png: 읽기 실패"]);
  assert.deepEqual(h.views, ["일부 결과"]);
  h.drop();
  assert.deepEqual(h.toasts, [dropToast]);
  assert.deepEqual(h.busy, [true]);
  notice.resolve();
  await job;
  assert.deepEqual(h.busy, [true, false]);
});

test("모두 읽지 못한 안내 중 예외가 나도 잠금을 푼다", async () => {
  const h = harness();
  const error = new Error("안내 실패");
  h.handlers.alert = async (message) => { h.alerts.push(message); throw error; };
  const job = h.start();
  h.pending.resolve({ view: "원래 결과", skipped: [{ name: "a.png", reason: "지원하지 않음" }] });
  await job;
  assert.deepEqual(h.alerts, ["파일을 붙이지 못했습니다. 편집 중인 프레임은 그대로입니다.\n\n· a.png: 지원하지 않음"]);
  assert.deepEqual(h.errors, [error]);
  assert.equal(blocksProjectChange(h.guard.current), false);
});

test("프로젝트가 바뀌면 옛 결과와 안내를 버리고 지금 내용을 다시 읽을 때까지 잠근다", async () => {
  const h = harness();
  const refreshed = Promise.withResolvers();
  let refreshCount = 0;
  h.handlers.projectView = () => { refreshCount++; return refreshed.promise; };
  const job = h.start();
  h.guard.current = invalidateAppend(h.guard.current);
  h.pending.resolve({ view: "옛 결과", skipped: [{ name: "a.png", reason: "옛 오류" }] });
  await Promise.resolve();
  assert.equal(refreshCount, 1);
  assert.deepEqual(h.views, []);
  assert.deepEqual(h.alerts, []);
  assert.deepEqual(h.busy, [true]);
  h.drop();
  assert.deepEqual(h.toasts, [dropToast]);
  refreshed.resolve("지금 결과");
  await job;
  assert.deepEqual(h.staleViews, ["지금 결과"]);
  assert.deepEqual(h.errors, []);
  assert.deepEqual(h.busy, [true, false]);
  assert.equal(blocksProjectChange(h.guard.current), false);
});

for (const phase of ["파일 읽기", "지금 내용 다시 읽기"]) {
  test(`프로젝트 변경 뒤 ${phase} 오류는 알리지 않고 잠금을 푼다`, async () => {
    const h = harness();
    const error = new Error("지난 작업 오류");
    h.handlers.projectView = async () => { throw error; };
    const job = h.start();
    h.guard.current = invalidateAppend(h.guard.current);
    if (phase === "파일 읽기") h.pending.reject(error);
    else h.pending.resolve({ view: "옛 결과", skipped: [] });
    await job;
    assert.deepEqual(h.views, []);
    assert.deepEqual(h.staleViews, []);
    assert.deepEqual(h.errors, []);
    assert.deepEqual(h.busy, [true, false]);
    assert.equal(blocksProjectChange(h.guard.current), false);
  });
}

test("붙이는 동안 새 녹화·열기에서 쓰는 차단 문구와 새 작업 대화상자 판정을 유지한다", async () => {
  const h = harness();
  assert.equal(appendBlockMessage(h.guard.current), null);
  const job = h.start();
  assert.equal(appendBlockMessage(h.guard.current), appendToast);
  for (const dialog of ["newBlank", "collage"]) assert.equal(isProjectDialog(dialog), true);
  for (const dialog of [null, "settings", "save", "manage", "resize"]) assert.equal(isProjectDialog(dialog), false);
  h.pending.resolve({ view: "결과", skipped: [] });
  await job;
  assert.equal(appendBlockMessage(h.guard.current), null);
});

test("붙이는 동안 Ctrl·Meta+O와 G만 차단 알림을 요청하고 다른 키도 실행은 막는다", async () => {
  const h = harness();
  const notified = [key("o", true), key("O", true), key("o", false, true), key("g"), key("G")];
  const silent = [key("o"), key("g", true), key("g", false, true), key("s", true), key("Delete"), key(" "), key("Escape")];
  for (const event of [...notified, ...silent]) assert.equal(appendShortcutAction(h.guard.current, event), "allow");
  const job = h.start();
  for (const event of notified) assert.equal(appendShortcutAction(h.guard.current, event), "notify");
  for (const event of silent) assert.equal(appendShortcutAction(h.guard.current, event), "block");
  h.pending.resolve({ view: "결과", skipped: [] });
  await job;
  for (const event of [...notified, ...silent]) assert.equal(appendShortcutAction(h.guard.current, event), "allow");
});
