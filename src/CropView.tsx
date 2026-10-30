// 자르기 화면: 미리보기 위에서 영역을 드래그로 고른다.

import { useEffect, useRef, useState } from "react";
import { api, FrameInfo, frameUrl, Rect } from "./api";
import { Check, isModalOpen } from "./dialogs";
import SidePanel from "./SidePanel";
import { Field, NumInput, PanelFoot, Section, useReportDirty } from "./Panel";

const RATIOS: Record<string, number | null> = {
  free: null,
  "1:1": 1,
  "4:3": 4 / 3,
  "3:4": 3 / 4,
  "16:9": 16 / 9,
  "9:16": 9 / 16,
};

interface Props {
  frame: FrameInfo;
  scale: number;
  onApply: (r: Rect) => void;
  onCancel: () => void;
  onError: (m: string) => void;
}

type Drag =
  | { kind: "new" | "corner"; ax: number; ay: number }
  | { kind: "move"; dx: number; dy: number };

export default function CropView({ frame, scale, onApply, onCancel, onError }: Props) {
  const W = frame.width;
  const H = frame.height;
  const [rect, setRect] = useState<Rect>(() => ({
    x: Math.round(W * 0.1),
    y: Math.round(H * 0.1),
    w: Math.round(W * 0.8),
    h: Math.round(H * 0.8),
  }));
  const [ratio, setRatio] = useState("free");
  const start = useRef(rect);
  const full = rect.x === 0 && rect.y === 0 && rect.w === W && rect.h === H;
  useReportDirty(JSON.stringify(rect) !== JSON.stringify(start.current));
  const [grid, setGrid] = useState(true);
  const drag = useRef<Drag | null>(null);
  const box = useRef<HTMLDivElement>(null);

  const toImage = (e: { clientX: number; clientY: number }) => {
    const r = box.current!.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(W, (e.clientX - r.left) / scale)),
      y: Math.max(0, Math.min(H, (e.clientY - r.top) / scale)),
    };
  };

  /** 고정점 a에서 p까지의 사각형 (비율 고정이면 맞춘다) */
  const span = (ax: number, ay: number, px: number, py: number): Rect => {
    let w = Math.abs(px - ax);
    let h = Math.abs(py - ay);
    const r = RATIOS[ratio];
    if (r) {
      if (w / Math.max(h, 1e-6) > r) w = h * r;
      else h = w / r;
    }
    let x = px < ax ? ax - w : ax;
    let y = py < ay ? ay - h : ay;
    x = Math.max(0, Math.min(x, W - w));
    y = Math.max(0, Math.min(y, H - h));
    return { x: Math.round(x), y: Math.round(y), w: Math.max(1, Math.round(w)), h: Math.max(1, Math.round(h)) };
  };

  const onDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const p = toImage(e);
    const near = 10 / scale;
    const corners = [
      [rect.x, rect.y, rect.x + rect.w, rect.y + rect.h],
      [rect.x + rect.w, rect.y, rect.x, rect.y + rect.h],
      [rect.x, rect.y + rect.h, rect.x + rect.w, rect.y],
      [rect.x + rect.w, rect.y + rect.h, rect.x, rect.y],
    ];
    // 모서리를 잡으면 반대쪽 모서리를 고정점으로
    const c = corners.find(([cx, cy]) => Math.abs(p.x - cx) < near && Math.abs(p.y - cy) < near);
    if (c) drag.current = { kind: "corner", ax: c[2], ay: c[3] };
    else if (p.x > rect.x && p.x < rect.x + rect.w && p.y > rect.y && p.y < rect.y + rect.h)
      drag.current = { kind: "move", dx: p.x - rect.x, dy: p.y - rect.y };
    else drag.current = { kind: "new", ax: p.x, ay: p.y };
  };

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      const p = toImage(e);
      if (d.kind === "move")
        setRect((r) => ({
          ...r,
          x: Math.round(Math.max(0, Math.min(W - r.w, p.x - d.dx))),
          y: Math.round(Math.max(0, Math.min(H - r.h, p.y - d.dy))),
        }));
      else setRect(span(d.ax, d.ay, p.x, p.y));
    };
    const onUp = () => (drag.current = null);
    const onKey = (e: KeyboardEvent) => {
      // 위에 확인·설정 같은 대화상자가 떠 있으면 그 창의 키다 (Esc·Enter가 도구까지 닫거나 적용하지 않게)
      if (isModalOpen()) return;
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (e.key === "Enter" && !full) onApply(rect);
      else if (e.key === "Escape") onCancel();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("keydown", onKey);
    };
  });

  const setField = (k: keyof Rect, v: number) => {
    if (!Number.isFinite(v)) return;
    setRect((r) => {
      const n = { ...r, [k]: Math.round(v) };
      n.x = Math.max(0, Math.min(n.x, W - 1));
      n.y = Math.max(0, Math.min(n.y, H - 1));
      n.w = Math.max(1, Math.min(n.w, W - n.x));
      n.h = Math.max(1, Math.min(n.h, H - n.y));
      return n;
    });
  };

  const autoFit = async () => {
    const b = await api.trimBounds();
    if (!b) return onError("투명한 테두리가 없습니다");
    const [x, y, w, h] = b;
    if (w === W && h === H) onError("투명한 테두리가 없습니다");
    setRect({ x, y, w, h });
  };

  const s = scale;
  return (
    <div className="crop">
      <SidePanel
        title="자르기"
        desc="미리보기에서 끌어 남길 영역을 정합니다. 모서리를 끌면 크기, 안쪽을 끌면 위치가 바뀝니다."
        foot={
          <PanelFoot
            target="전체 프레임 (모든 프레임을 같은 영역으로 자름)"
            summary={
              <>
                {W} × {H} → <b>{rect.w} × {rect.h}</b>
              </>
            }
            applyLabel="자르기 (Enter)"
            disabledReason={full ? "자를 영역이 화면 전체와 같습니다" : null}
            onCancel={onCancel}
            onApply={() => onApply(rect)}
          />
        }
      >
        <Section title="위치 (왼쪽 위 모서리)">
          <Field label="X">
            <NumInput value={rect.x} min={0} unit="px" wide label="X 위치" onChange={(v) => setField("x", v)} />
          </Field>
          <Field label="Y">
            <NumInput value={rect.y} min={0} unit="px" wide label="Y 위치" onChange={(v) => setField("y", v)} />
          </Field>
        </Section>
        <Section title="크기">
          <Field label="가로">
            <NumInput value={rect.w} min={1} unit="px" wide label="가로" onChange={(v) => setField("w", v)} />
          </Field>
          <Field label="세로">
            <NumInput value={rect.h} min={1} unit="px" wide label="세로" onChange={(v) => setField("h", v)} />
          </Field>
          <Field label="비율">
            <select
              aria-label="가로세로 비율"
              value={ratio}
              onChange={(e) => {
                setRatio(e.target.value);
                const r = RATIOS[e.target.value];
                if (r) setRect((c) => span(c.x, c.y, c.x + Math.min(c.w, c.h * r), c.y + Math.min(c.h, c.w / r)));
              }}
            >
              {Object.keys(RATIOS).map((k) => (
                <option key={k} value={k}>
                  {k === "free" ? "자유" : k}
                </option>
              ))}
            </select>
          </Field>
        </Section>
        <Section title="빠르게 맞추기">
          <div className="row">
            <button onClick={autoFit} title="모든 프레임에서 투명한 바깥 테두리만 걷어낸 영역">
              자동 맞춤
            </button>
            <button onClick={() => setRect({ x: 0, y: 0, w: W, h: H })}>전체 선택</button>
          </div>
          <p className="hint">자동 맞춤은 모든 프레임을 겹쳐 보아 바깥쪽의 투명한 테두리만 걷어냅니다. 투명한 테두리가 없으면 그대로입니다.</p>
        </Section>
        <Section title="보기">
          <Check label="3등분 격자" checked={grid} onChange={setGrid} />
        </Section>
      </SidePanel>
      <div className="crop-area">
        <div className="crop-box" ref={box} style={{ width: W * s, height: H * s }} onMouseDown={onDown}>
          <img src={frameUrl(frame.id)} width={W * s} height={H * s} draggable={false} />
          <div
            className="crop-sel"
            style={{ left: rect.x * s, top: rect.y * s, width: rect.w * s, height: rect.h * s }}
          >
            {grid && (
              <>
                <i className="gv" style={{ left: "33.33%" }} />
                <i className="gv" style={{ left: "66.66%" }} />
                <i className="gh" style={{ top: "33.33%" }} />
                <i className="gh" style={{ top: "66.66%" }} />
              </>
            )}
            <b className="h nw" />
            <b className="h ne" />
            <b className="h sw" />
            <b className="h se" />
          </div>
        </div>
      </div>
    </div>
  );
}
