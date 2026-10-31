// 줌 애니메이션 화면: 확대할 영역을 고르면, 지금 프레임부터 그 영역이 커지며 보였다가 돌아간다.

import { useEffect, useRef, useState } from "react";
import { api, errText, FrameInfo, frameUrl, Region, ZoomFx, ZoomScale } from "./api";
import { Check, isModalOpen } from "./dialogs";
import SidePanel from "./SidePanel";
import { Field, NumInput, PanelFoot, Section, Slider, useReportDirty } from "./Panel";

const SCALES: [ZoomScale, string][] = [
  ["goodFit", "적당한 크기"],
  ["half", "최대 크기의 절반"],
  ["threeQuarter", "최대 크기의 75%"],
  ["full", "꽉 차게"],
];

interface Props {
  frame: FrameInfo;
  /** 지금 프레임 번호 (0부터). 효과는 여기서 시작한다. */
  index: number;
  /** 지금 프레임부터 끝까지 남은 시간(ms) */
  remainMs: number;
  scale: number;
  onApply: (zoom: ZoomFx) => void;
  onCancel: () => void;
  onError: (m: string) => void;
}

type Drag = { kind: "new"; ax: number; ay: number } | { kind: "move"; dx: number; dy: number };

export default function ZoomView({ frame, index, remainMs, scale, onApply, onCancel, onError }: Props) {
  const W = frame.width;
  const H = frame.height;
  const [region, setRegion] = useState<Region>(() => ({
    x: Math.round(W * 0.35),
    y: Math.round(H * 0.35),
    w: Math.round(W * 0.3),
    h: Math.round(H * 0.3),
    ellipse: false,
  }));
  const [zoomScale, setZoomScale] = useState<ZoomScale>("goodFit");
  const [toCenter, setToCenter] = useState(true);
  const [shadow, setShadow] = useState(true);
  const [spread, setSpread] = useState(60);
  const [totalSec, setTotalSec] = useState(2);
  const [introMs, setIntroMs] = useState(300);
  const [outroMs, setOutroMs] = useState(300);
  const [showResult, setShowResult] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  const drag = useRef<Drag | null>(null);
  const box = useRef<HTMLDivElement>(null);

  const zoom: ZoomFx = {
    region,
    scale: zoomScale,
    toCenter,
    shadow,
    spread,
    totalMs: Math.max(100, Math.round((Number.isFinite(totalSec) ? totalSec : 2) * 1000)),
    introMs: Math.max(0, Math.round(introMs) || 0),
    outroMs: Math.max(0, Math.round(outroMs) || 0),
  };
  const key = JSON.stringify(zoom);
  const startKey = useRef(key);
  useReportDirty(key !== startKey.current);
  const actualMs = Math.min(zoom.totalMs, remainMs);
  const valid = Number.isFinite(totalSec) && totalSec > 0;

  // 결과 미리보기 (가장 크게 확대된 순간)
  useEffect(() => {
    if (!showResult) return;
    let url: string | null = null;
    let alive = true;
    const t = window.setTimeout(async () => {
      try {
        const buf = await api.zoomPreview(frame.id, zoom);
        if (!alive) return;
        url = URL.createObjectURL(new Blob([buf], { type: "image/png" }));
        setPreview(url);
      } catch (e) {
        onError(errText(e));
      }
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
      if (url) window.setTimeout(() => URL.revokeObjectURL(url!), 2000);
    };
  }, [key, frame.id, showResult]);

  useEffect(() => {
    const toImage = (e: MouseEvent) => {
      const r = box.current!.getBoundingClientRect();
      return {
        x: Math.max(0, Math.min(W, (e.clientX - r.left) / scale)),
        y: Math.max(0, Math.min(H, (e.clientY - r.top) / scale)),
      };
    };
    const onMove = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      const p = toImage(e);
      if (d.kind === "move")
        setRegion((r) => ({
          ...r,
          x: Math.round(Math.max(0, Math.min(W - r.w, p.x - d.dx))),
          y: Math.round(Math.max(0, Math.min(H - r.h, p.y - d.dy))),
        }));
      else
        setRegion((r) => ({
          ...r,
          x: Math.round(Math.min(d.ax, p.x)),
          y: Math.round(Math.min(d.ay, p.y)),
          w: Math.max(4, Math.round(Math.abs(p.x - d.ax))),
          h: Math.max(4, Math.round(Math.abs(p.y - d.ay))),
        }));
    };
    const onUp = () => (drag.current = null);
    const onKey = (e: KeyboardEvent) => {
      // 위에 확인·설정 같은 대화상자가 떠 있으면 그 창의 키다 (Esc·Enter가 도구까지 닫거나 적용하지 않게)
      if (isModalOpen()) return;
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("keydown", onKey);
    };
  }, [W, H, scale, onCancel]);

  const onDown = (e: React.MouseEvent) => {
    if (e.button !== 0 || showResult) return;
    e.preventDefault();
    const r = box.current!.getBoundingClientRect();
    const x = (e.clientX - r.left) / scale;
    const y = (e.clientY - r.top) / scale;
    const inside = x > region.x && x < region.x + region.w && y > region.y && y < region.y + region.h;
    drag.current = inside ? { kind: "move", dx: x - region.x, dy: y - region.y } : { kind: "new", ax: x, ay: y };
  };

  return (
    <div className="tool">
      <div className="tool-area">
        <div
          className="tool-box"
          ref={box}
          style={{ width: W * scale, height: H * scale, cursor: showResult ? "default" : "crosshair" }}
          onMouseDown={onDown}
        >
          <img
            src={showResult && preview ? preview : frameUrl(frame.id)}
            width={W * scale}
            height={H * scale}
            draggable={false}
          />
          {!showResult && (
            <div
              className="region zoom-region"
              style={{ left: region.x * scale, top: region.y * scale, width: region.w * scale, height: region.h * scale }}
            />
          )}
        </div>
      </div>

      <SidePanel
        title="줌 애니메이션"
        desc="미리보기에서 끌어 확대할 영역을 고릅니다 (영역 안쪽을 끌면 옮겨짐)."
        foot={
          <PanelFoot
            target={`지금 프레임(${index + 1}번)부터 ${(actualMs / 1000).toFixed(1)}초 동안`}
            disabledReason={!valid ? "총 시간을 0보다 크게 입력하세요" : null}
            onCancel={onCancel}
            onApply={() => onApply(zoom)}
          />
        }
      >
        <Section title="미리보기">
          <Check label="가장 크게 확대된 순간 보기" checked={showResult} onChange={setShowResult} />
        </Section>
        <Section title="확대 영역">
          <p className="hint">
            위치 {region.x}, {region.y} · 크기 {region.w} × {region.h} px
          </p>
          <Field label="배율">
            <select aria-label="확대 배율" value={zoomScale} onChange={(e) => setZoomScale(e.target.value as ZoomScale)}>
              {SCALES.map(([k, l]) => (
                <option key={k} value={k}>
                  {l}
                </option>
              ))}
            </select>
          </Field>
        </Section>
        <Section title="모양">
          <Check label="화면 가운데로 옮기며 확대" checked={toCenter} onChange={setToCenter} />
          <Check label="그림자" checked={shadow} onChange={setShadow} />
          <Field label="확산" sub>
            <Slider value={spread} min={0} max={100} unit="%" label="그림자 확산" disabled={!shadow} onChange={setSpread} />
          </Field>
        </Section>
        <Section title="시간">
          <Field label="총 시간">
            <NumInput value={totalSec} min={0.1} step={0.1} unit="초" wide label="총 시간" onChange={setTotalSec} />
          </Field>
          <Field label="커지기" sub>
            <NumInput value={introMs} min={0} unit="ms" wide label="커지는 데 걸리는 시간" onChange={setIntroMs} />
          </Field>
          <Field label="돌아오기" sub>
            <NumInput value={outroMs} min={0} unit="ms" wide label="원래대로 돌아오는 데 걸리는 시간" onChange={setOutroMs} />
          </Field>
          {valid && zoom.totalMs > remainMs && (
            <p className="pwarn">
              요청 {(zoom.totalMs / 1000).toFixed(1)}초 → 실제 {(remainMs / 1000).toFixed(1)}초. 지금 프레임부터 끝까지 남은 길이만큼만
              들어갑니다.
            </p>
          )}
        </Section>
      </SidePanel>
    </div>
  );
}
