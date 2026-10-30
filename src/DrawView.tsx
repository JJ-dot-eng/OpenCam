// 그리기 화면 (D): 펜으로 그린 그림을 지금 프레임·선택 구간·전체에 덧씌운다.

import { useContext, useEffect, useRef, useState } from "react";
import { IconColorPicker, IconEraser, IconPencil } from "@tabler/icons-react";
import { FrameInfo, frameUrl } from "./api";
import { Check, isModalOpen } from "./dialogs";
import SidePanel from "./SidePanel";
import { ColorInput, Field, FrameContext, PanelFoot, Scope, ScopePicker, scopeSel, Section, Segmented, Slider, useReportDirty } from "./Panel";

interface Stroke {
  points: [number, number][];
  color: string;
  size: number;
  opacity: number;
  soft: boolean;
  shadow: boolean;
  erase: boolean;
}

interface Props {
  frame: FrameInfo;
  scale: number;
  /** target: 덧씌울 프레임 번호 (빈 목록이면 전체, 여러 장이면 처음~끝 구간) */
  onApply: (rgba: Uint8Array, w: number, h: number, target: number[]) => void;
  onCancel: () => void;
}

/** 획 하나를 그린다. 한 획 안에서 겹친 부분이 진해지지 않게 따로 그린 뒤 투명도를 한 번에 준다. */
function paintStroke(ctx: CanvasRenderingContext2D, s: Stroke, w: number, h: number) {
  const tmp = document.createElement("canvas");
  tmp.width = w;
  tmp.height = h;
  const t = tmp.getContext("2d")!;
  t.lineCap = "round";
  t.lineJoin = "round";
  t.strokeStyle = s.color;
  t.fillStyle = s.color;
  t.lineWidth = s.size;
  if (s.soft) t.filter = `blur(${Math.max(1, s.size / 4)}px)`;
  if (s.shadow && !s.erase) {
    t.shadowColor = "rgba(0,0,0,0.6)";
    t.shadowBlur = s.size;
    t.shadowOffsetX = s.size / 3;
    t.shadowOffsetY = s.size / 3;
  }
  t.beginPath();
  const [x0, y0] = s.points[0];
  if (s.points.length === 1) {
    t.arc(x0, y0, s.size / 2, 0, Math.PI * 2);
    t.fill();
  } else {
    t.moveTo(x0, y0);
    for (const [x, y] of s.points.slice(1)) t.lineTo(x, y);
    t.stroke();
  }
  ctx.save();
  ctx.globalAlpha = s.erase ? 1 : s.opacity;
  ctx.globalCompositeOperation = s.erase ? "destination-out" : "source-over";
  ctx.drawImage(tmp, 0, 0);
  ctx.restore();
}

export default function DrawView({ frame, scale, onApply, onCancel }: Props) {
  const frames = useContext(FrameContext);
  const W = frame.width;
  const H = frame.height;
  const canvas = useRef<HTMLCanvasElement>(null);
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const live = useRef<Stroke | null>(null);
  const [color, setColor] = useState("#ff3b30");
  const [size, setSize] = useState(6);
  const [opacity, setOpacity] = useState(100);
  const [soft, setSoft] = useState(false);
  const [shadow, setShadow] = useState(false);
  const [tool, setTool] = useState<"pen" | "erase" | "pick">("pen");
  const [scope, setScope] = useState<Scope>("current");
  useReportDirty(strokes.length > 0);

  const redraw = (extra?: Stroke | null) => {
    const ctx = canvas.current?.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, W, H);
    for (const s of strokes) paintStroke(ctx, s, W, H);
    if (extra) paintStroke(ctx, extra, W, H);
  };

  useEffect(() => redraw());

  const toImage = (e: { clientX: number; clientY: number }): [number, number] => {
    const r = canvas.current!.getBoundingClientRect();
    return [(e.clientX - r.left) / scale, (e.clientY - r.top) / scale];
  };

  /** 스포이드: 지금 프레임에서 색을 읽는다 */
  const pick = (x: number, y: number) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      const c = document.createElement("canvas");
      c.width = 1;
      c.height = 1;
      const ctx = c.getContext("2d")!;
      ctx.drawImage(img, Math.floor(x), Math.floor(y), 1, 1, 0, 0, 1, 1);
      const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
      setColor("#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join(""));
      setTool("pen");
    };
    img.src = frameUrl(frame.id);
  };

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!live.current) return;
      live.current.points.push(toImage(e));
      redraw(live.current);
    };
    const onUp = () => {
      const s = live.current;
      live.current = null;
      if (s) setStrokes((all) => [...all, s]);
    };
    const onKey = (e: KeyboardEvent) => {
      // 위에 확인·설정 같은 대화상자가 떠 있으면 그 창의 키다 (Esc·Enter가 도구까지 닫거나 적용하지 않게)
      if (isModalOpen()) return;
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (e.key === "Escape") onCancel();
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        setStrokes((all) => all.slice(0, -1));
      }
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

  const apply = () => {
    const ctx = canvas.current!.getContext("2d")!;
    const data = ctx.getImageData(0, 0, W, H).data;
    onApply(new Uint8Array(data.buffer, data.byteOffset, data.byteLength), W, H, scopeSel(scope, frames));
  };

  return (
    <div className="tool">
      <div className="tool-area">
        <div className="tool-box" style={{ width: W * scale, height: H * scale }}>
          <img src={frameUrl(frame.id)} width={W * scale} height={H * scale} draggable={false} />
          <canvas
            ref={canvas}
            width={W}
            height={H}
            className="draw-canvas"
            style={{ width: W * scale, height: H * scale, cursor: tool === "pick" ? "copy" : "crosshair" }}
            onMouseDown={(e) => {
              if (e.button !== 0) return;
              e.preventDefault();
              const p = toImage(e);
              if (tool === "pick") return pick(p[0], p[1]);
              live.current = { points: [p], color, size, opacity: opacity / 100, soft, shadow, erase: tool === "erase" };
              redraw(live.current);
            }}
          />
        </div>
      </div>

      <SidePanel
        title="그리기"
        desc="미리보기 위에 마우스로 그립니다."
        foot={
          <PanelFoot
            scope={<ScopePicker value={scope} onChange={setScope} span />}
            summary={strokes.length > 0 && `그린 획 ${strokes.length}개`}
            disabledReason={strokes.length === 0 ? "먼저 미리보기 위에 그리세요" : null}
            onCancel={onCancel}
            onApply={apply}
          />
        }
      >
        <Section title="도구">
          <Segmented
            label="그리기 도구"
            value={tool}
            onChange={setTool}
            options={[
              ["pen", "펜", <IconPencil size={16} />],
              ["erase", "지우개", <IconEraser size={16} />],
              ["pick", "스포이드", <IconColorPicker size={16} />],
            ]}
          />
          <p className="hint">
            {tool === "pen" ? "끌어서 그립니다." : tool === "erase" ? "그린 획을 지웁니다 (원래 화면은 지워지지 않음)." : "미리보기를 눌러 그 색을 펜 색으로 고릅니다."}
          </p>
        </Section>
        <Section title="펜 모양">
          <Field label="색">
            <ColorInput label="펜 색" value={color} onChange={setColor} />
          </Field>
          <Field label="굵기">
            <Slider value={size} min={1} max={80} unit="px" label="굵기" onChange={setSize} />
          </Field>
          <Field label="불투명도">
            <Slider value={opacity} min={5} max={100} unit="%" label="불투명도" onChange={setOpacity} />
          </Field>
          <Check label="부드러운 펜 (가장자리 흐림)" checked={soft} onChange={setSoft} />
          <Check label="그림자" checked={shadow} onChange={setShadow} />
        </Section>
        <Section title="그린 획">
          <div className="row">
            <button disabled={strokes.length === 0} onClick={() => setStrokes((a) => a.slice(0, -1))}>
              한 획 취소 (Ctrl+Z)
            </button>
            <button disabled={strokes.length === 0} onClick={() => setStrokes([])}>
              모두 지우기
            </button>
          </div>
          <p className="hint">적용하기 전까지는 원래 프레임이 바뀌지 않습니다.</p>
        </Section>
      </SidePanel>
    </div>
  );
}
