import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

// 웹뷰를 띄우지 않고 실제 버튼·단축키와 검사 함수의 연결도 확인한다.
const source = ts.createSourceFile(
  "Editor.tsx", readFileSync(new URL("../src/Editor.tsx", import.meta.url), "utf8"),
  ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX,
);

function findAll(node, predicate) {
  const found = [];
  function visit(current) {
    if (predicate(current)) found.push(current);
    ts.forEachChild(current, visit);
  }
  visit(node);
  return found;
}

function callback(name) {
  const declarations = findAll(source, (node) => ts.isVariableDeclaration(node) && node.name.getText(source) === name);
  assert.equal(declarations.length, 1, name);
  const call = declarations[0].initializer;
  assert.ok(ts.isCallExpression(call) && call.expression.getText(source) === "useCallback", name);
  return call.arguments[0];
}

function calls(node, name) {
  return findAll(node, (item) => ts.isCallExpression(item) && item.expression.getText(source) === name);
}

test("새 녹화·열기·새로 만들기 버튼은 처리 중일 때 모두 비활성화된다", () => {
  const controls = findAll(source, (node) =>
    (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
    ["button", "select"].includes(node.tagName.getText(source)),
  );
  const projectControls = controls.filter((node) => node.attributes.properties.some((attr) =>
    ts.isJsxAttribute(attr) && ["onClick", "onChange"].includes(attr.name.getText(source)) &&
    /\b(backToRecorder|openPaths|openNewDialog)\b/.test(attr.initializer?.getText(source) ?? ""),
  ));
  assert.equal(projectControls.length, 7, "위쪽 세 곳과 빈 화면 네 곳을 모두 확인합니다");
  for (const control of projectControls) {
    const disabled = control.attributes.properties.find((attr) => ts.isJsxAttribute(attr) && attr.name.getText(source) === "disabled");
    assert.ok(disabled && ts.isJsxExpression(disabled.initializer), control.getText(source));
    assert.equal(disabled.initializer.expression.getText(source), "busy");
  }
});

test("새 녹화·열기는 첫 단계부터 잠금을 확인하고 빈 영상·콜라주도 같은 검사에 연결된다", () => {
  for (const name of ["backToRecorder", "openPaths"]) {
    const first = callback(name).body.statements[0];
    assert.ok(ts.isIfStatement(first), name);
    assert.equal(first.expression.getText(source), "blockWhileAppending()");
    assert.ok(ts.isReturnStatement(first.thenStatement), name);
  }
  const first = callback("openNewDialog").body.statements[0];
  assert.ok(ts.isIfStatement(first));
  assert.equal(calls(first.expression, "isProjectDialog").length, 1);
  assert.equal(calls(first.expression, "blockWhileAppending").length, 1);
  assert.ok(ts.isReturnStatement(first.thenStatement));
  const blocker = callback("blockWhileAppending");
  assert.equal(calls(blocker, "appendBlockMessage").length, 1);
  assert.equal(calls(blocker, "showToast")[0].arguments[0].getText(source), "message");
});

test("단축키는 입력 칸을 먼저 제외하고 붙이기 검사를 처리 중 검사보다 먼저 한다", () => {
  const declarations = findAll(source, (node) => ts.isVariableDeclaration(node) &&
    node.name.getText(source) === "onKey" && calls(node, "appendShortcutAction").length > 0);
  assert.equal(declarations.length, 1);
  const statements = declarations[0].initializer.body.statements;
  const inputCheck = statements.findIndex((node) => ts.isIfStatement(node) && node.expression.getText(source).includes('tag === "INPUT"'));
  const appendCheck = statements.findIndex((node) => calls(node, "appendShortcutAction").length > 0);
  const busyCheck = statements.findIndex((node) => ts.isIfStatement(node) && node.expression.getText(source).startsWith("busy ||"));
  assert.ok(inputCheck >= 0 && inputCheck < appendCheck && appendCheck < busyCheck);
  const blocked = statements[appendCheck + 1];
  assert.equal(blocked.expression.getText(source), 'appendAction !== "allow"');
  const notify = blocked.thenStatement.statements[0];
  assert.equal(notify.expression.getText(source), 'appendAction === "notify"');
  assert.equal(calls(notify, "e.preventDefault").length, 1);
  assert.equal(calls(notify, "blockWhileAppending").length, 1);
  assert.ok(ts.isReturnStatement(blocked.thenStatement.statements.at(-1)));
});
