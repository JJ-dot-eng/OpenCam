import test from "node:test";
import assert from "node:assert/strict";
import {
  beginAppend,
  blocksProjectChange,
  createAppendGuard,
  finishAppend,
  invalidateAppend,
  isCurrentAppend,
} from "../src/appendGuard.ts";

test("첫 붙이기는 받고 같은 순간의 두 번째 붙이기는 막는다", () => {
  const idle = createAppendGuard();
  assert.equal(blocksProjectChange(idle), false);
  const first = beginAppend(idle);
  assert.ok(first);
  assert.equal(blocksProjectChange(first.guard), true);
  assert.equal(beginAppend(first.guard), null);
  assert.equal(isCurrentAppend(first.guard, first.job), true);
  assert.equal(blocksProjectChange(idle), false);
});

test("붙이기가 끝나면 프로젝트 바꾸기와 다음 붙이기를 허용한다", () => {
  const first = beginAppend(createAppendGuard());
  const idle = finishAppend(first.guard, first.job);
  assert.equal(blocksProjectChange(idle), false);
  assert.equal(isCurrentAppend(idle, first.job), false);
  const second = beginAppend(idle);
  assert.ok(second.job.id > first.job.id);
  assert.equal(isCurrentAppend(second.guard, first.job), false);
  assert.equal(isCurrentAppend(second.guard, second.job), true);
  assert.deepEqual(finishAppend(second.guard, first.job), second.guard);
});

test("프로젝트가 바뀌면 옛 결과를 버리고 끝날 때까지 중복 붙이기를 막는다", () => {
  const first = beginAppend(createAppendGuard());
  const changed = invalidateAppend(first.guard);
  assert.equal(isCurrentAppend(changed, first.job), false);
  assert.equal(blocksProjectChange(changed), true);
  assert.equal(beginAppend(changed), null);
  const idle = finishAppend(changed, first.job);
  assert.equal(blocksProjectChange(idle), false);
  const second = beginAppend(idle);
  assert.equal(isCurrentAppend(second.guard, second.job), true);
  assert.equal(isCurrentAppend(second.guard, first.job), false);
});

for (const outcome of ["성공", "실패", "예외"]) {
  test(`붙이기 ${outcome} 뒤 finally에서 잠금을 푼다`, async () => {
    let guard = createAppendGuard();
    const started = beginAppend(guard);
    guard = started.guard;
    try {
      await Promise.resolve();
      if (outcome === "실패") await Promise.reject(new Error("읽기 실패"));
      if (outcome === "예외") throw new Error("결과 처리 실패");
    } catch (e) {
      assert.ok(e instanceof Error);
    } finally {
      guard = finishAppend(guard, started.job);
    }
    assert.equal(blocksProjectChange(guard), false);
    assert.ok(beginAppend(guard));
  });
}
