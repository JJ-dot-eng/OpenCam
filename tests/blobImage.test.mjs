import test from "node:test";
import assert from "node:assert/strict";
import { createBlobImageLoader } from "../src/blobImage.ts";

// 브라우저 대신 쓰는 가짜 URL·그림. 만든 URL과 해제한 URL을 기록한다.
function harness() {
  const created = [];
  const revoked = [];
  const images = [];
  const loader = createBlobImageLoader({
    createUrl: (blob) => {
      const url = `blob:test/${created.length + 1}`;
      created.push([url, blob]);
      return url;
    },
    revokeUrl: (url) => revoked.push(url),
    makeImage: () => {
      const img = { onload: null, onerror: null, src: "" };
      images.push(img);
      return img;
    },
  });
  const results = [];
  const load = (blob = new Blob(["png"])) =>
    loader.load(
      blob,
      (img) => results.push(["성공", img]),
      () => results.push(["실패"]),
    );
  return { loader, created, revoked, images, results, load };
}

test("그림을 다 읽으면 URL을 해제하고 그림을 넘긴다", () => {
  const h = harness();
  const blob = new Blob(["png"]);
  h.load(blob);
  assert.equal(h.created.length, 1);
  assert.equal(h.created[0][1], blob);
  assert.equal(h.images[0].src, h.created[0][0]);
  assert.deepEqual(h.revoked, []);
  assert.equal(h.loader.pending(), 1);
  const img = h.images[0];
  img.onload();
  assert.deepEqual(h.revoked, [h.created[0][0]]);
  assert.deepEqual(h.results, [["성공", img]]);
  assert.equal(h.loader.pending(), 0);
});

test("그림을 못 읽어도 URL을 해제하고 실패를 알린다", () => {
  const h = harness();
  h.load();
  h.images[0].onerror();
  assert.deepEqual(h.revoked, [h.created[0][0]]);
  assert.deepEqual(h.results, [["실패"]]);
  assert.equal(h.loader.pending(), 0);
});

test("성공과 실패가 겹쳐 와도 URL은 한 번만 해제하고 결과도 한 번만 넘긴다", () => {
  const h = harness();
  h.load();
  const { onload, onerror } = h.images[0];
  onload();
  onerror();
  onload();
  assert.equal(h.revoked.length, 1);
  assert.equal(h.results.length, 1);
  assert.equal(h.results[0][0], "성공");
  // 끝난 뒤에는 그림의 콜백도 떼어 둔다
  assert.equal(h.images[0].onload, null);
  assert.equal(h.images[0].onerror, null);
});

test("화면이 닫히면 읽는 중인 URL을 해제하고 뒤늦은 결과는 버린다", () => {
  const h = harness();
  h.load();
  h.load();
  const [a, b] = h.images.map((img) => ({ onload: img.onload, onerror: img.onerror }));
  h.loader.dispose();
  assert.deepEqual(h.revoked.sort(), h.created.map(([url]) => url).sort());
  assert.equal(h.loader.pending(), 0);
  a.onload();
  b.onerror();
  assert.deepEqual(h.results, []);
  assert.equal(h.revoked.length, 2);
  // 두 번 닫아도 다시 해제하지 않는다
  h.loader.dispose();
  assert.equal(h.revoked.length, 2);
});

test("닫힌 뒤에 들어온 읽기는 URL을 만들지 않는다", () => {
  const h = harness();
  h.loader.dispose();
  h.load();
  assert.equal(h.created.length, 0);
  assert.equal(h.images.length, 0);
  assert.deepEqual(h.results, []);
});

test("여러 개를 읽으면 끝난 것만 해제하고 나머지는 닫을 때 해제한다", () => {
  const h = harness();
  h.load();
  h.load();
  h.load();
  h.images[1].onload();
  assert.deepEqual(h.revoked, [h.created[1][0]]);
  assert.equal(h.loader.pending(), 2);
  h.loader.dispose();
  assert.equal(h.revoked.length, 3);
  assert.equal(new Set(h.revoked).size, 3);
  assert.equal(h.results.length, 1);
});
