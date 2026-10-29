// 아래쪽 프레임 목록. 프레임이 수천 장이어도 보이는 칸만 그린다.
// 썸네일을 끌어 다른 자리에 놓으면 프레임을 옮긴다 (선택한 여러 장이면 함께).

import { useEffect, useRef, useState } from "react";
import { FrameInfo, frameUrl } from "./api";

export type ThumbSize = "small" | "medium" | "large";
/** 썸네일 칸 크기(px). w는 간격 8px을 뺀 칸 너비. 작게면 한 화면에 보통의 약 1.5배가 들어간다 */
export const THUMB: Record<ThumbSize, { w: number; h: number }> = {
  small: { w: 80, h: 76 },
  medium: { w: 124, h: 112 },
  large: { w: 180, h: 150 },
};

interface Props {
  frames: FrameInfo[];
  current: number;
  sel: number[];
  onSelect: (i: number, e: { ctrlKey: boolean; shiftKey: boolean }) => void;
  /** 프레임 시간을 소수점 3째 자리까지 */
  threeDigit: boolean;
  size: ThumbSize;
  /** 오른쪽 클릭: 프레임 번호와 화면 좌표 */
  onMenu: (i: number, x: number, y: number) => void;
  /** 끌어 놓기: i번을 잡고 to번 자리(그 프레임 앞, 맨 뒤는 프레임 수)에 놓았다 */
  onMove: (i: number, to: number) => void;
}

/** 이만큼(px) 움직여야 끌기로 본다. 그냥 누르기와 구분하려고 */
const DRAG_START = 6;

export default function FrameStrip({ frames, current, sel, onSelect, threeDigit, size, onMenu, onMove }: Props) {
  const { w: itemW, h: itemH } = THUMB[size];
  const ITEM = itemW + 8;
  const ref = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ left: 0, width: 1000 });
  const selected = new Set(sel);
  /** 끌기 중이면 놓을 자리 (0..프레임 수) */
  const [dropAt, setDropAt] = useState<number | null>(null);

  /** 썸네일을 누른다. 이미 고른 프레임을 그냥 누르면 끌어서 여러 장을 함께 옮길 수 있게 선택은 뗄 때 바꾼다. */
  const onItemDown = (i: number, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const keepSel = selected.has(i) && !e.ctrlKey && !e.shiftKey;
    if (!keepSel) onSelect(i, e);
    const el = ref.current;
    if (!el) return;
    const startX = e.clientX;
    let to: number | null = null;
    const target = (x: number) => {
      const r = el.getBoundingClientRect();
      return Math.max(0, Math.min(frames.length, Math.round((x - r.left + el.scrollLeft) / ITEM)));
    };
    const move = (ev: MouseEvent) => {
      if (to === null && Math.abs(ev.clientX - startX) < DRAG_START) return;
      // 가장자리로 끌면 그쪽으로 스크롤
      const r = el.getBoundingClientRect();
      if (ev.clientX < r.left + 40) el.scrollLeft -= 20;
      else if (ev.clientX > r.right - 40) el.scrollLeft += 20;
      to = target(ev.clientX);
      setDropAt(to);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setDropAt(null);
      if (to !== null) onMove(i, to);
      else if (keepSel) onSelect(i, { ctrlKey: false, shiftKey: false });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => setView({ left: el.scrollLeft, width: el.clientWidth });
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    el.addEventListener("scroll", update, { passive: true });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", update);
    };
  }, []);

  // 지금 프레임이 보이도록 스크롤
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const x = current * ITEM;
    if (x < el.scrollLeft) el.scrollLeft = x;
    else if (x + ITEM > el.scrollLeft + el.clientWidth) el.scrollLeft = x + ITEM - el.clientWidth;
  }, [current, ITEM]);

  // 세로 휠로도 가로 스크롤
  const onWheel = (e: React.WheelEvent) => {
    if (ref.current && Math.abs(e.deltaY) > Math.abs(e.deltaX)) ref.current.scrollLeft += e.deltaY;
  };

  // 프레임이 갑자기 줄면(요약·삭제) 스크롤 위치가 아직 예전 값일 수 있어 목록 길이로 막는다
  const first = Math.min(frames.length, Math.max(0, Math.floor(view.left / ITEM) - 2));
  const last = Math.min(frames.length, Math.ceil((view.left + view.width) / ITEM) + 2);
  let start = 0;
  for (let i = 0; i < first; i++) start += frames[i].delayMs;

  const items = [];
  for (let i = first; i < last; i++) {
    const f = frames[i];
    items.push(
      <div
        key={f.id}
        className={`strip-item ${selected.has(i) ? "sel" : ""} ${i === current ? "cur" : ""}`}
        style={{ left: i * ITEM, width: itemW, height: itemH }}
        onMouseDown={(e) => onItemDown(i, e)}
        onContextMenu={(e) => {
          e.preventDefault();
          onMenu(i, e.clientX, e.clientY);
        }}
        title={`${i + 1}번 · ${f.delayMs}ms · 시작 ${(start / 1000).toFixed(2)}초`}
      >
        <div className="strip-thumb">
          <img src={frameUrl(f.id, true)} draggable={false} loading="lazy" />
        </div>
        <div className="strip-meta">
          <span>{i + 1}</span>
          <span>{(f.delayMs / 1000).toFixed(threeDigit ? 3 : 2)}초</span>
        </div>
      </div>,
    );
    start += f.delayMs;
  }

  return (
    <div className="strip" ref={ref} onWheel={onWheel} style={{ height: itemH + 16 }}>
      <div className="strip-inner" style={{ width: frames.length * ITEM }}>
        {items}
        {dropAt !== null && <div className="strip-drop" style={{ left: dropAt * ITEM - 5 }} />}
      </div>
    </div>
  );
}
