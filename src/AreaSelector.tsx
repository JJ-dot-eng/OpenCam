import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import { selectionRect, type Point } from "./areaGeometry";
import "./area-selector.css";

const win = getCurrentWindow();

export default function AreaSelector() {
  const start = useRef<Point | null>(null);
  const busy = useRef(false);
  // 비동기 크기 조회가 끝나기 전에 취소하거나 새 선택을 시작하면 옛 결과를 버린다.
  const generation = useRef(0);
  const [selection, setSelection] = useState<{ a: Point; b: Point } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hint, setHint] = useState<Point | null>(null);
  const cancel = async () => {
    const current = ++generation.current;
    busy.current = true;
    try { await api.finishAreaSelection(null); }
    catch (e) { setError(`취소하지 못했습니다: ${String(e)}`); }
    finally {
      if (generation.current === current) { start.current = null; setSelection(null); busy.current = false; }
    }
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.preventDefault(); void cancel(); } };
    const onVisibility = () => {
      if (document.hidden) {
        ++generation.current;
        start.current = null;
        busy.current = false;
        setSelection(null);
        setError(null);
      }
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("visibilitychange", onVisibility);
    return () => { window.removeEventListener("keydown", onKey); document.removeEventListener("visibilitychange", onVisibility); };
  }, []);

  const point = (e: React.PointerEvent): Point => ({
    x: Math.max(0, Math.min(window.innerWidth, e.clientX)),
    y: Math.max(0, Math.min(window.innerHeight, e.clientY)),
  });
  const left = selection ? Math.min(selection.a.x, selection.b.x) : 0;
  const top = selection ? Math.min(selection.a.y, selection.b.y) : 0;
  const width = selection ? Math.abs(selection.a.x - selection.b.x) : 0;
  const height = selection ? Math.abs(selection.a.y - selection.b.y) : 0;

  return <div className="area-overlay"
    onPointerDown={(e) => {
      if (e.button !== 0 || busy.current) return;
      const p = point(e); start.current = p; setError(null); setSelection({ a: p, b: p });
      e.currentTarget.setPointerCapture(e.pointerId);
    }}
    onPointerMove={(e) => {
      const p = point(e);
      if (start.current) setSelection({ a: start.current, b: p });
      else setHint({ x: Math.max(0, Math.min(window.innerWidth - 320, p.x + 18)), y: Math.max(0, Math.min(window.innerHeight - 45, p.y + 18)) });
    }}
    onPointerCancel={() => { start.current = null; setSelection(null); }}
    onPointerUp={async (e) => {
      const a = start.current;
      if (!a || busy.current) return;
      const b = point(e); start.current = null;
      if (Math.abs(a.x - b.x) < 2 || Math.abs(a.y - b.y) < 2) { setSelection(null); return; }
      const current = ++generation.current;
      busy.current = true;
      try {
        const cssWidth = window.innerWidth, cssHeight = window.innerHeight;
        const [origin, size] = await Promise.all([win.innerPosition(), win.innerSize()]);
        // WebView의 배율과 OS DPI가 다를 수도 있으므로 실제 물리 크기와 CSS 뷰포트 크기로 변환한다.
        const rect = selectionRect(a, b, { width: cssWidth, height: cssHeight }, origin, size);
        if (generation.current !== current) return;
        await api.finishAreaSelection(rect);
      } catch (e) {
        if (generation.current === current) setError(`영역을 선택하지 못했습니다: ${String(e)} · Esc 취소`);
      } finally {
        if (generation.current === current) { setSelection(null); busy.current = false; }
      }
    }}>
    <div className="area-hint" style={hint ? { left: hint.x, top: hint.y, transform: "none" } : undefined}>{error ?? "드래그하여 녹화 영역을 선택하세요 · Esc 취소"}</div>
    {selection && <div className="area-box" style={{ left, top, width, height }} />}
  </div>;
}
