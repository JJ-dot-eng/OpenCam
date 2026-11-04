// 아래쪽 프레임 목록. 프레임이 수천 장이어도 보이는 칸만 그린다.
// 썸네일을 끌어 프레임을 옮기는 기능은 없앴다(모르고 여러 장이 옮겨지는 일이 있어서). 순서는 잘라내기·붙여넣기로 바꾼다.
// 도구 화면이 열려 있으면 썸네일 위에 적용 범위 띠를 그린다. 양 끝을 끌면 범위가, 가운데를 끌면 자리가 바뀐다.
// Ctrl+휠로 줄이면 프레임 한 장이 칸 하나보다 좁아진다(축소 보기). 그때는 칸마다 그 자리 프레임 하나를 보여 주고,
// 선택과 지금 프레임은 그 위에 색으로 칠한다.
// 썸네일 바로 위에는 효과 기록을 효과 종류마다 한 줄씩 그린다(도구를 열지 않아도). 기록 띠를 누르면 그 구간을 고른다.

import { useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Icon, IconAdjustmentsHorizontal, IconPencil, IconTransitionRight, IconTypography, IconZoomInArea } from "@tabler/icons-react";
import { FrameInfo, frameUrl } from "./api";
import { TimelineContext } from "./Panel";
import { Band, BandEdge, dragBand, EffectKey, effectRows, prefixMs, runs, spanLabel } from "./timeline";

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
  /** 열린 도구의 적용 범위 (없으면 띠 줄을 그리지 않는다) */
  band?: Band | null;
  /** 효과 기록 띠를 눌렀다: 그 구간(처음~끝 프레임 번호) */
  onPickRange?: (start: number, end: number) => void;
  /** 축소 보기의 지금 프레임 선(파란 선)을 끌었다: 그 프레임으로 (선택은 그대로) */
  onSeek?: (i: number) => void;
}

/** 범위 띠 줄 높이(px). 썸네일은 그만큼 아래로 내려간다 */
const TRACK = 24;
/** 효과 기록 한 줄 높이(px) */
const ROW = 19;
/** 효과 기록 띠에 붙는 아이콘 (왼쪽 도구 목록과 같은 것) */
const HIST_ICON: Record<EffectKey, Icon> = {
  filter: IconAdjustmentsHorizontal,
  draw: IconPencil,
  insert: IconTypography,
  transition: IconTransitionRight,
  zoom: IconZoomInArea,
};
/** Ctrl+휠 한 번에 바뀌는 배율 */
const ZOOM_STEP = 1.25;

export default function FrameStrip({ frames, current, sel, onSelect, threeDigit, size, onMenu, band, onPickRange, onSeek }: Props) {
  const { w: itemW, h: itemH } = THUMB[size];
  const ITEM = itemW + 8;
  const ref = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ left: 0, width: 1000 });
  const selected = useMemo(() => new Set(sel), [sel]);
  // 프레임마다 시작 시각(ms): 썸네일 시간·기록 띠 길이를 매번 더하지 않게 한 번만 만든다
  const { delays } = useContext(TimelineContext);
  const startMs = useMemo(() => prefixMs(delays), [delays]);

  // 프레임 한 장의 폭(px). 가장 넓으면 칸 하나, 가장 좁으면 모든 프레임이 한 화면에 들어가는 폭
  const [unitWant, setUnitWant] = useState(ITEM);
  const minUnit = Math.min(ITEM, view.width / Math.max(1, frames.length));
  const unit = Math.max(minUnit, Math.min(ITEM, unitWant));
  const compact = unit < ITEM - 0.01;
  /** 프레임 사이 빈칸: 보통은 8px, 축소 보기에서는 없음 */
  const gap = compact ? 0 : 8;
  const unitNow = useRef(unit);
  unitNow.current = unit;
  const minNow = useRef(minUnit);
  minNow.current = minUnit;
  /** Ctrl+휠로 배율을 바꾼 뒤, 마우스 밑에 있던 프레임 자리를 다시 마우스 밑으로 */
  const zoomAnchor = useRef<{ at: number; x: number } | null>(null);

  /** 화면 x좌표 밑의 프레임 번호 */
  const indexAt = (x: number) => {
    const el = ref.current!;
    const p = (x - el.getBoundingClientRect().left + el.scrollLeft) / unit;
    return Math.max(0, Math.min(frames.length - 1, Math.floor(p)));
  };

  /** 썸네일을 누른다: 고르기만 한다 (Ctrl·Shift로 여러 장) */
  const onItemDown = (i: number, e: React.MouseEvent) => {
    if (e.button === 0) onSelect(i, e);
  };

  /**
   * 썸네일 줄 위에서 끌기: 마우스 밑 프레임 번호가 바뀔 때마다 onIndex를 부르고, 가장자리로 끌면 그쪽으로 스크롤한다.
   * 손을 떼면 onUp. 파란 선 끌기와 범위 띠 끌기가 함께 쓴다.
   */
  const startIndexDrag = (e: React.MouseEvent, onIndex: (i: number) => void, onUp?: () => void) => {
    const el = ref.current;
    if (!el || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    let last = indexAt(e.clientX);
    const move = (ev: MouseEvent) => {
      const r = el.getBoundingClientRect();
      if (ev.clientX < r.left + 40) el.scrollLeft -= 20;
      else if (ev.clientX > r.right - 40) el.scrollLeft += 20;
      const i = indexAt(ev.clientX);
      if (i === last) return;
      last = i;
      onIndex(i);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onUp?.();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** 지금 프레임 선(파란 선)을 끈다: 마우스 밑 프레임으로 옮긴다 (재생 위치 막대처럼) */
  const onSeekDown = (e: React.MouseEvent) => startIndexDrag(e, (i) => onSeek?.(i));

  /**
   * 범위 띠의 끝이나 가운데를 끈다. 바뀔 때마다 바로 알리고(done=false),
   * 손을 떼면 마지막 범위를 한 번 더 알린다(done=true, 이때만 미리보기 그림을 바꾼다).
   */
  const onBandDown = (edge: BandEdge, e: React.MouseEvent) => {
    if (!band) return;
    const orig = { start: band.start, end: band.end };
    const { onChange, minLen } = band;
    const from = indexAt(e.clientX);
    let last = orig;
    startIndexDrag(
      e,
      (i) => {
        const next = dragBand(edge, orig, edge === "move" ? i - from : i, frames.length, minLen);
        if (next.start === last.start && next.end === last.end) return;
        last = next;
        onChange(next.start, next.end, edge, false);
      },
      () => {
        if (last.start !== orig.start || last.end !== orig.end) onChange(last.start, last.end, edge, true);
      },
    );
  };

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => setView({ left: el.scrollLeft, width: el.clientWidth });
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    el.addEventListener("scroll", update, { passive: true });
    // 휠: 그냥 굴리면 가로 스크롤, Ctrl을 누르고 굴리면 확대·축소.
    // 웹뷰 기본 동작(페이지 확대)을 막아야 해서 passive가 아닌 리스너를 직접 붙인다
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) {
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) el.scrollLeft += e.deltaY;
        return;
      }
      e.preventDefault();
      const u = unitNow.current;
      const next = Math.max(minNow.current, Math.min(ITEM, u * (e.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP)));
      if (next === u) return;
      const x = e.clientX - el.getBoundingClientRect().left;
      zoomAnchor.current = { at: (x + el.scrollLeft) / u, x };
      setUnitWant(next);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", update);
      el.removeEventListener("wheel", onWheel);
    };
  }, [ITEM]);

  useLayoutEffect(() => {
    const a = zoomAnchor.current;
    zoomAnchor.current = null;
    if (a && ref.current) ref.current.scrollLeft = a.at * unit - a.x;
  }, [unit]);

  // 지금 프레임이 보이도록 스크롤
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const u = unitNow.current;
    const x = current * u;
    const w = Math.max(u, 2);
    if (x < el.scrollLeft) el.scrollLeft = x;
    else if (x + w > el.scrollLeft + el.clientWidth) el.scrollLeft = x + w - el.clientWidth;
  }, [current]);

  const n = frames.length;
  // 효과 기록: 효과 종류마다 한 줄 (쓴 종류만). 범위 띠 줄 아래, 썸네일 바로 위. 띠마다 설명 글도 여기서 한 번만 만든다
  const rows = useMemo(
    () =>
      effectRows(frames.map((f) => f.tags)).map((r) => ({
        ...r,
        titles: r.spans.map((x) => `${r.label} · ${spanLabel(x, startMs[x.end + 1] - startMs[x.start])} (누르면 이 구간을 고릅니다)`),
      })),
    [frames, startMs],
  );
  const selRuns = useMemo(() => runs(sel), [sel]);
  const markRuns = useMemo(() => (band?.marks ? runs(band.marks) : []), [band?.marks]);
  /** a~b 프레임이 지금 보이는 곳에 걸치나 (pad만큼 넉넉하게) */
  const visible = (a: number, b: number, pad = 0) => (b + 1) * unit >= view.left - pad && a * unit <= view.left + view.width + pad;
  const histTop = band ? 4 + TRACK : 4;
  const top = histTop + rows.length * ROW + (rows.length ? 4 : 2);
  const items = [];
  const overlays = [];
  if (!compact) {
    // 프레임이 갑자기 줄면(요약·삭제) 스크롤 위치가 아직 예전 값일 수 있어 목록 길이로 막는다
    const first = Math.min(n, Math.max(0, Math.floor(view.left / ITEM) - 2));
    const last = Math.min(n, Math.ceil((view.left + view.width) / ITEM) + 2);
    for (let i = first; i < last; i++) {
      const f = frames[i];
      items.push(
        <div
          key={f.id}
          className={`strip-item ${selected.has(i) ? "sel" : ""} ${i === current ? "cur" : ""}`}
          style={{ left: i * ITEM, top, width: itemW, height: itemH }}
          onMouseDown={(e) => onItemDown(i, e)}
          onContextMenu={(e) => {
            e.preventDefault();
            onMenu(i, e.clientX, e.clientY);
          }}
          title={`${i + 1}번 · ${f.delayMs}ms · 시작 ${((startMs[i] ?? 0) / 1000).toFixed(2)}초`}
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
    }
  } else {
    // 축소 보기: 칸(ITEM px)마다 그 가운데에 있는 프레임 하나를 보여 준다. 누르면 마우스 밑 프레임을 고른다.
    const innerW = n * unit;
    const firstTile = Math.max(0, Math.floor(view.left / ITEM) - 1);
    const lastTile = Math.min(Math.ceil(innerW / ITEM), Math.ceil((view.left + view.width) / ITEM) + 1);
    for (let k = firstTile; k < lastTile; k++) {
      const x = k * ITEM;
      const w = Math.min(itemW, innerW - x);
      if (w < 24) continue;
      const i = Math.min(n - 1, Math.floor((x + w / 2) / unit));
      const f = frames[i];
      items.push(
        <div
          key={k}
          className="strip-item compact"
          style={{ left: x, top, width: w, height: itemH }}
          onMouseDown={(e) => onItemDown(indexAt(e.clientX), e)}
          onContextMenu={(e) => {
            e.preventDefault();
            onMenu(indexAt(e.clientX), e.clientX, e.clientY);
          }}
        >
          <div className="strip-thumb">
            <img src={frameUrl(f.id, true)} draggable={false} loading="lazy" />
          </div>
          <div className="strip-meta">
            <span>{i + 1}</span>
            <span>{((startMs[i] ?? 0) / 1000).toFixed(threeDigit ? 3 : 2)}초</span>
          </div>
        </div>,
      );
    }
    for (const r of selRuns) {
      if (!visible(r.start, r.end)) continue;
      overlays.push(
        <div key={`s${r.start}`} className="strip-selrun" style={{ left: r.start * unit, width: Math.max(2, (r.end - r.start + 1) * unit), top, height: itemH }} />,
      );
    }
    if (current < n)
      overlays.push(
        <div
          key="cur"
          className="strip-curline"
          style={{ left: current * unit, width: Math.max(3, unit), top, height: itemH }}
          onMouseDown={onSeekDown}
          title="끌어서 보는 위치를 옮깁니다"
        />,
      );
  }

  // 띠는 프레임 자리에 맞춰 그린다 (프레임이 줄어든 직후에도 목록 안으로)
  const last1 = n - 1;
  const bs = band ? Math.max(0, Math.min(last1, band.start)) : 0;
  const be = band ? Math.max(bs, Math.min(last1, band.end)) : 0;
  const span = (a: number, b: number) => Math.max(compact ? 2 : 0, (b - a + 1) * unit - gap);

  return (
    <div className="strip-wrap">
      <div className="strip" ref={ref} style={{ height: itemH + 10 + top }}>
        <div className="strip-inner" style={{ width: n * unit }}>
          {band && (
            <div className="strip-track">
              <div
                className="band"
                style={{ left: bs * unit, width: Math.max(18, span(bs, be)) }}
                onMouseDown={(e) => onBandDown("move", e)}
                title="양 끝을 끌어 범위를 바꾸고, 가운데를 끌어 옮깁니다"
              >
                {band.parts?.map((p, k) => (
                  <div
                    key={k}
                    className="band-part"
                    style={{ left: (Math.max(bs, p.start) - bs) * unit, width: span(Math.max(bs, p.start), Math.min(be, p.end)) }}
                    title={p.label}
                  />
                ))}
                {/* 떨어진 프레임 표시: 이어진 구간으로 묶고 보이는 것만 그린다 (수천 장을 골라도 가볍게) */}
                {markRuns
                  .filter((m) => visible(m.start, m.end, 50))
                  .map((m) => (
                    <div key={m.start} className="band-mark" style={{ left: (m.start - bs) * unit, width: Math.max(2, span(m.start, m.end)) }} />
                  ))}
                <span className="band-label">{band.label}</span>
                <div className="band-edge start" onMouseDown={(e) => onBandDown("start", e)} />
                <div className="band-edge end" onMouseDown={(e) => onBandDown("end", e)} />
              </div>
            </div>
          )}
          {rows.map((r, k) => (
            <div key={r.key} className="hist-row" style={{ top: histTop + k * ROW }}>
              {r.spans.map((x, q) =>
                !visible(x.start, x.end, 50) ? null : (
                  <div
                    key={x.start}
                    className="hist-span"
                    style={{ left: x.start * unit, width: Math.max(3, span(x.start, x.end)) }}
                    title={r.titles[q]}
                    onMouseDown={(e) => e.stopPropagation()}
                    onClick={() => onPickRange?.(x.start, x.end)}
                  >
                    {/* 띠마다 아이콘과 효과 이름 (좁으면 잘리고, 마우스를 대면 전체가 보인다) */}
                    <HistIcon kind={r.key} />
                    <span className="hist-text">{r.label}</span>
                  </div>
                ),
              )}
            </div>
          ))}
          {items}
          {overlays}
        </div>
      </div>
      {compact && (
        <button
          className="strip-zoom"
          onClick={() => {
            // 지금 프레임을 가운데에 두고 원래 크기로
            zoomAnchor.current = { at: current + 0.5, x: view.width / 2 };
            setUnitWant(ITEM);
          }} title="Ctrl+휠로 확대·축소. 누르면 원래 크기로">
          축소 보기 {Math.round((unit / ITEM) * 100)}% · 원래 크기
        </button>
      )}
    </div>
  );
}

function HistIcon({ kind }: { kind: EffectKey }) {
  const I = HIST_ICON[kind];
  return <I className="hist-icon" size={11} stroke={1.8} />;
}
