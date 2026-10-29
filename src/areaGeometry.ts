import type { Rect } from "./api";

export type Point = { x: number; y: number };
type Size = { width: number; height: number };

/** CSS 화면 좌표를 선택 창의 실제 물리 픽셀과 화면 위치로 변환한다. */
export function selectionRect(a: Point, b: Point, viewport: Size, origin: Point, physical: Size): Rect {
  if (!(viewport.width > 0 && viewport.height > 0 && physical.width > 0 && physical.height > 0))
    throw new Error("선택 창의 크기를 읽을 수 없습니다");
  const sx = physical.width / viewport.width, sy = physical.height / viewport.height;
  const x1 = Math.round(Math.min(a.x, b.x) * sx), y1 = Math.round(Math.min(a.y, b.y) * sy);
  const x2 = Math.round(Math.max(a.x, b.x) * sx), y2 = Math.round(Math.max(a.y, b.y) * sy);
  return { x: origin.x + x1, y: origin.y + y1, w: x2 - x1, h: y2 - y1 };
}
