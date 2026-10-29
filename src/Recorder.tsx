// 녹화 틀 창: 가운데가 뚫린 틀 + 툴바.
// 구멍은 Rust 쪽에서 SetWindowRgn으로 실제로 잘라내므로, 그 부분은 클릭도 아래 창으로 넘어간다.

import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import { emitTo, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  IconCamera,
  IconCheck,
  IconChevronDown,
  IconFolderOpen,
  IconGripVertical,
  IconMarquee2,
  IconMenu2,
  IconMinus,
  IconMovie,
  IconPlayerPause,
  IconPlayerStopFilled,
  IconPointer,
  IconSettings,
  IconX,
} from "@tabler/icons-react";
import { api, errText, formatMs, CaptureWindow, RecStatus, Rect, Settings } from "./api";
import "./recorder.css";

const TOOLBAR = 40; // CSS px
const BORDER = 5; // CSS px
/** 툴바가 다 보이는 최소 너비 (CSS px, tauri.conf.json의 minWidth와 같게). 틀이 더 좁으면 툴바만 이만큼 넓다. */
const MIN_TOOLBAR = 400;
const FPS_LIST = [5, 10, 15, 20, 25, 30, 40, 50, 60];

type ResizeDir = "North" | "South" | "East" | "West" | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";

interface Layout {
  dpr: number;
  /** 틀 바깥 너비 (물리 픽셀). 창보다 좁을 수 있다. */
  frameW: number;
  /** 구멍 (물리 픽셀, 창 기준) */
  hole: Rect;
  /** 툴바 높이 (물리 픽셀) */
  t: number;
  /** 테두리 두께 (물리 픽셀) */
  b: number;
}

const win = getCurrentWindow();

export default function Recorder() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [layout, setLayout] = useState<Layout | null>(null);
  const [status, setStatus] = useState<RecStatus | null>(null);
  const [fullMonitor, setFullMonitor] = useState<Rect | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [resizing, setResizing] = useState(false);
  const [sizeDraft, setSizeDraft] = useState<{ w: string; h: string } | null>(null);
  // FPS 직접 입력 중이면 숫자 칸을 보여준다 (녹화 틀은 작아서 대화상자를 띄울 수 없다)
  const [fpsDraft, setFpsDraft] = useState<string | null>(null);
  const [windowChoices, setWindowChoices] = useState<CaptureWindow[] | null>(null);
  const accel = useRef(0);
  // 틀을 툴바보다 좁게 만든 경우의 틀 너비(물리 픽셀). null이면 창 너비 그대로.
  const frameOverride = useRef<number | null>(null);
  // 알림은 이 버튼 오른쪽부터 덮는다 (녹화·일시정지는 늘 누를 수 있게)
  const pauseBtn = useRef<HTMLButtonElement>(null);
  const bottom = settings?.recToolbarBottom ?? false;
  const recording = status?.recording ?? false;

  const showNotice = useCallback((m: string) => {
    setNotice(m);
    // 긴 알림(오류 내용 등)은 읽을 시간을 더 준다
    window.setTimeout(() => setNotice((cur) => (cur === m ? null : cur)), Math.max(5000, m.length * 90));
  }, []);

  // 창 크기에서 구멍 위치를 계산해 Rust에 알린다.
  // toolbarBottom: 툴바 위치 설정을 막 바꿔 아직 화면에 반영되지 않았을 때 직접 넘긴다.
  const relayout = useCallback(async (toolbarBottom = bottom) => {
    const dpr = await win.scaleFactor();
    const size = await win.innerSize();
    const b = Math.round(BORDER * dpr);
    const t = Math.round(TOOLBAR * dpr);
    const minTb = Math.round(MIN_TOOLBAR * dpr);
    // 사용자가 창을 끌어 다른 크기로 바꿨으면 좁은 틀 설정은 끝
    if (frameOverride.current !== null && size.width !== Math.max(frameOverride.current, minTb))
      frameOverride.current = null;
    const frameW = Math.min(size.width, frameOverride.current ?? size.width);
    const hole: Rect = {
      x: b,
      y: toolbarBottom ? b : t + b,
      w: Math.max(1, frameW - 2 * b),
      h: Math.max(1, size.height - t - 2 * b),
    };
    const frameTop = toolbarBottom ? 0 : t;
    const keep: Rect[] = [
      { x: 0, y: toolbarBottom ? size.height - t : 0, w: size.width, h: t },
      { x: 0, y: frameTop, w: frameW, h: size.height - t },
    ];
    setLayout({ dpr, frameW, hole, t, b });
    await api.setHole(hole, keep);
  }, [bottom]);

  useEffect(() => {
    api.getSettings().then(setSettings);
    api.recStatus().then(setStatus);
    api.getFullCapture().then(setFullMonitor);
    const un = [
      listen<RecStatus>("rec-status", (e) => setStatus(e.payload)),
      listen<string>("notice", (e) => showNotice(e.payload)),
      listen("settings-changed", () => api.getSettings().then(setSettings)),
    ];
    return () => un.forEach((p) => p.then((f) => f()));
  }, [showNotice]);

  useEffect(() => {
    relayout();
    const un = [win.onResized(() => relayout()), win.onScaleChanged(() => relayout())];
    return () => un.forEach((p) => p.then((f) => f()));
  }, [relayout]);

  useEffect(() => {
    if (!resizing) return;
    let done = false;
    let timer = 0;
    const check = async () => {
      try {
        const down = await api.resizeMouseDown();
        if (done) return;
        if (down) timer = window.setTimeout(check, 75);
        else setResizing(false);
      } catch {
        if (!done) setResizing(false);
      }
    };
    timer = window.setTimeout(check, 75);
    return () => { done = true; window.clearTimeout(timer); };
  }, [resizing]);

  const disableFull = useCallback(async () => {
    if (!fullMonitor) return;
    await api.setFullCapture(null);
    setFullMonitor(null);
  }, [fullMonitor]);

  /** 구멍 크기를 w×h(물리 픽셀)로 맞춘다. 틀이 툴바보다 좁으면 창은 툴바 너비로 두고 틀만 좁게. */
  const setHoleSize = useCallback(
    async (w: number, h: number) => {
      if (!layout) return;
      await disableFull();
      const { t, b, dpr } = layout;
      const frameW = Math.max(16, Math.round(w)) + 2 * b;
      const minTb = Math.round(MIN_TOOLBAR * dpr);
      frameOverride.current = frameW < minTb ? frameW : null;
      await win.setSize(new PhysicalSize(Math.max(frameW, minTb), Math.max(16, Math.round(h)) + t + 2 * b));
      // 창 크기가 그대로면 크기 바뀜 알림이 안 오므로 직접 다시 계산
      await relayout();
    },
    [layout, relayout, disableFull],
  );

  /** 구멍의 화면 위치를 (x, y)로 맞춘다 */
  const setHolePos = useCallback(
    async (x: number, y: number) => {
      if (!layout) return;
      await win.setPosition(new PhysicalPosition(Math.round(x - layout.hole.x), Math.round(y - layout.hole.y)));
    },
    [layout],
  );

  const saveSettings = useCallback(
    async (next: Settings) => {
      setSettings(next);
      try {
        await api.setSettings(next);
      } catch (e) {
        showNotice(errText(e));
      }
    },
    [showNotice],
  );

  const toggleRecord = useCallback(async () => {
    try {
      if (recording) await api.recStop();
      else await api.recStart();
    } catch (e) {
      showNotice(errText(e));
    }
  }, [recording, showNotice]);

  const saveRecordingImage = useCallback(async () => {
    try {
      showNotice(`이미지를 저장했습니다: ${await api.recSaveImage()}`);
    } catch (e) {
      showNotice(errText(e));
    }
  }, [showNotice]);

  const fitMonitor = useCallback(
    async (div: number) => {
      const mons = await api.monitors();
      const pos = await win.outerPosition();
      const m = mons.find((r) => pos.x >= r.x && pos.x < r.x + r.w && pos.y >= r.y && pos.y < r.y + r.h) ?? mons[0];
      if (!m) return;
      const w = Math.floor(m.w / div);
      const h = Math.floor(m.h / div) - (layout ? layout.t + 2 * layout.b : 0);
      await setHoleSize(w, h);
      await setHolePos(m.x + Math.floor((m.w - w) / 2), m.y + Math.floor((m.h - h) / 2));
    },
    [layout, setHoleSize, setHolePos],
  );

  const fitFullMonitor = useCallback(async () => {
    try {
      const mons = await api.monitors();
      const pos = await win.innerPosition();
      const m = mons.find((r) => pos.x >= r.x && pos.x < r.x + r.w && pos.y >= r.y && pos.y < r.y + r.h) ?? mons[0];
      if (!m) throw new Error("모니터를 찾을 수 없습니다");
      await disableFull();
      frameOverride.current = null;
      await win.setPosition(new PhysicalPosition(m.x, m.y));
      await win.setSize(new PhysicalSize(m.w, m.h));
      await win.setPosition(new PhysicalPosition(m.x, m.y));
      await relayout();
      const actualPos = await win.innerPosition(), actualSize = await win.innerSize();
      if (actualPos.x !== m.x || actualPos.y !== m.y || actualSize.width !== m.w || actualSize.height !== m.h)
        throw new Error("녹화 틀을 모니터 크기에 맞추지 못했습니다");
      await api.setFullCapture(m);
      setFullMonitor(m);
      showNotice("모니터 전체를 녹화합니다");
    } catch (e) { showNotice(errText(e)); }
  }, [disableFull, relayout, showNotice]);

  const chooseWindow = useCallback(async () => {
    if (recording) return;
    try {
      const choices = await api.captureWindows();
      if (!choices.length) return showNotice("녹화할 수 있는 창이 없습니다");
      setWindowChoices(choices);
    } catch (e) {
      showNotice(errText(e));
    }
  }, [recording, showNotice]);

  const fitRect = useCallback(async (r: Rect, activated = true) => {
    try {
      await disableFull();
      // 먼저 대상 모니터로 이동하여 그 모니터의 배율을 얻는다.
      await win.setPosition(new PhysicalPosition(r.x, r.y));
      const dpr = await win.scaleFactor();
      const b = Math.round(BORDER * dpr);
      const t = Math.round(TOOLBAR * dpr);
      const frameW = r.w + 2 * b;
      const minTb = Math.round(MIN_TOOLBAR * dpr);
      // 최대화한 창이나 화면 맨 위 영역은 툴바가 모니터 밖으로 나간다. 그러면 반대쪽에 붙인다.
      const mons = await api.monitors();
      const overlap = (m: Rect) =>
        Math.max(0, Math.min(m.x + m.w, r.x + r.w) - Math.max(m.x, r.x)) *
        Math.max(0, Math.min(m.y + m.h, r.y + r.h) - Math.max(m.y, r.y));
      const mon = mons.reduce<Rect | null>((best, m) => (!best || overlap(m) > overlap(best) ? m : best), null);
      const fitsTop = !mon || r.y - b - t >= mon.y;
      const fitsBottom = !mon || r.y + r.h + b + t <= mon.y + mon.h;
      let toolbarBottom = bottom;
      let toolbarHidden = false;
      if (bottom ? !fitsBottom : !fitsTop) {
        if (bottom ? fitsTop : fitsBottom) {
          toolbarBottom = !bottom;
          if (settings) await saveSettings({ ...settings, recToolbarBottom: toolbarBottom });
        } else toolbarHidden = true;
      }
      frameOverride.current = frameW < minTb ? frameW : null;
      await win.setSize(new PhysicalSize(Math.max(frameW, minTb), r.h + t + 2 * b));
      await win.setPosition(new PhysicalPosition(r.x - b, r.y - (toolbarBottom ? b : t + b)));
      await relayout(toolbarBottom);
      const size = await win.innerSize();
      const pos = await win.innerPosition();
      const scale = await win.scaleFactor();
      const actualB = Math.round(BORDER * scale);
      const actualT = Math.round(TOOLBAR * scale);
      const actualW = Math.min(size.width, frameOverride.current ?? size.width) - 2 * actualB;
      const actualH = size.height - actualT - 2 * actualB;
      if (pos.x + actualB !== r.x || pos.y + (toolbarBottom ? actualB : actualT + actualB) !== r.y || actualW !== r.w || actualH !== r.h)
        showNotice("창 크기 또는 위치가 제한되어 정확히 맞추지 못했습니다. 녹화 영역을 확인하세요.");
      else if (toolbarHidden)
        showNotice(`툴바를 놓을 자리가 없어 화면 밖에 있습니다. ${settings?.hotkeyRecord || "녹화 단축키"}로 녹화하거나 크기 메뉴의 '전체 화면'을 쓰세요.`);
      else if (!activated) showNotice("창에 맞췄지만 앞으로 가져오지 못했습니다. 가린 창이 없는지 확인하세요.");
      else if (toolbarBottom !== bottom)
        showNotice(`툴바가 화면 밖으로 나가서 ${toolbarBottom ? "아래" : "위"}로 옮겼습니다 (설정 '툴바를 아래에')`);
      else showNotice("선택한 영역에 녹화 틀을 맞췄습니다");
    } catch (e) {
      showNotice(errText(e));
    }
  }, [bottom, settings, saveSettings, relayout, showNotice, disableFull]);

  const fitWindow = useCallback(async (target: CaptureWindow) => {
    try {
      const { rect, activated } = await api.captureWindowBounds(target.id, target.pid);
      await fitRect(rect, activated);
    } catch (e) { showNotice(errText(e)); }
  }, [fitRect, showNotice]);

  const chooseArea = useCallback(async () => {
    if (recording) return;
    try { await api.beginAreaSelection(); }
    catch (e) { showNotice(errText(e)); }
  }, [recording, showNotice]);

  useEffect(() => {
    const un = listen<Rect>("area-selected", (e) => { void fitRect(e.payload); });
    return () => { void un.then((f) => f()); };
  }, [fitRect]);

  // 키보드: 방향키 이동, Ctrl+1~4 위치 불러오기, Ctrl+Shift+1~4 위치 저장
  useEffect(() => {
    const onKey = async (e: KeyboardEvent) => {
      if (!settings || !layout) return;
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (!recording && e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        if (!e.repeat) await chooseWindow();
        return;
      }
      if (!recording && e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        if (!e.repeat) await chooseArea();
        return;
      }
      if (recording && e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (!e.repeat && !(settings.recGlobalSnapshotHotkey && await api.recSnapshotGlobalActive()))
          await saveRecordingImage();
        return;
      }
      const arrows: Record<string, [number, number]> = {
        ArrowLeft: [-1, 0],
        ArrowRight: [1, 0],
        ArrowUp: [0, -1],
        ArrowDown: [0, 1],
      };
      const dir = arrows[e.key];
      if (dir) {
        if (fullMonitor) return;
        if (recording && settings.ignoreArrowKeysWhileRecording) return;
        e.preventDefault();
        accel.current = e.repeat && settings.recArrowKeyAccel ? Math.min(accel.current + 1, 40) : 0;
        const step = 1 + Math.floor(accel.current / 2);
        const pos = await win.outerPosition();
        await win.setPosition(new PhysicalPosition(pos.x + dir[0] * step, pos.y + dir[1] * step));
        return;
      }
      const n = Number(e.key) - 1;
      if (e.ctrlKey && n >= 0 && n < 4 && !recording) {
        e.preventDefault();
        const positions = [...settings.savedPositions];
        while (positions.length < 4) positions.push(null);
        if (e.shiftKey) {
          const pos = await win.outerPosition();
          positions[n] = [pos.x + layout.hole.x, pos.y + layout.hole.y, layout.hole.w, layout.hole.h];
          await saveSettings({ ...settings, savedPositions: positions });
          showNotice(`위치 ${n + 1}에 저장했습니다`);
        } else if (positions[n]) {
          const [x, y, w, h] = positions[n]!;
          await setHoleSize(w, h);
          await setHolePos(x, y);
        } else {
          showNotice(`저장된 위치 ${n + 1}이 없습니다 (Ctrl+Shift+${n + 1}로 저장)`);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settings, layout, recording, saveSettings, setHoleSize, setHolePos, showNotice, saveRecordingImage, chooseWindow, chooseArea, fullMonitor]);

  /** 편집 창에서 파일 열기. 녹화 틀은 숨었다가, 취소하면 다시 나온다. */
  const openFiles = async () => {
    await win.hide();
    const main = await WebviewWindow.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await emitTo("main", "open-files");
  };

  /** ☰ 메뉴: 편집 창에서 대화상자를 연다. 녹화 틀은 숨었다가, 취소하면 다시 나온다. */
  const openMenu = async (name: string) => {
    await win.hide();
    const main = await WebviewWindow.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await emitTo("main", "open-dialog", name);
  };

  const openSettings = async () => {
    const main = await WebviewWindow.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await emitTo("main", "open-settings");
  };

  if (!layout || !settings) return null;
  const { dpr, hole, t, b } = layout;
  const css = (v: number) => v / dpr;
  const frameTop = bottom ? 0 : css(t);
  const frameStyle = {
    top: frameTop,
    left: 0,
    width: css(layout.frameW),
    height: css(hole.h + 2 * b),
    borderWidth: css(b),
    "--b": `${css(b)}px`,
  } as React.CSSProperties;
  const grip = (dir: ResizeDir) => (e: React.MouseEvent) => {
    if (e.button !== 0 || recording || fullMonitor) return;
    e.preventDefault();
    if (settings.useSemiTransparency) setResizing(true);
    if (settings.useMagnifier) void api.startMagnifier(layout.dpr).catch((error) => showNotice(errText(error)));
    void win.startResizeDragging(dir).catch((error) => { setResizing(false); showNotice(errText(error)); });
  };
  const state = recording ? (status?.paused ? "paused" : "recording") : "idle";
  // 틀 너비에 따라 툴바를 줄인다. 좁으면 덜 쓰는 것(커서·MP4 스위치, 열기, 설정)은 ☰ 메뉴로,
  // 아주 좁으면 영역·크기·FPS도 ☰ 메뉴와 단축키로만 쓴다. (기준은 각 단계 버튼들이 다 들어가는 너비)
  const compact = css(layout.frameW) < 760;
  const wide = css(layout.frameW) >= 1080;
  const presetValue = settings.sizePresets.findIndex(([w, h]) => w === hole.w && h === hole.h);
  const captureSize = fullMonitor ?? hole;
  const drag = fullMonitor ? undefined : "";

  const commitSize = () => {
    if (!sizeDraft) return;
    const w = parseInt(sizeDraft.w, 10);
    const h = parseInt(sizeDraft.h, 10);
    setSizeDraft(null);
    if (w > 0 && h > 0) setHoleSize(w, h);
  };

  return (
    <div className={`rec ${state} ${resizing ? "resizing" : ""}`}>
      <div className={`rec-toolbar ${bottom ? "bottom" : ""}`} style={{ height: css(t) }} data-tauri-drag-region={drag}
        onMouseDownCapture={(e) => {
          if (e.button === 0 && !fullMonitor && settings.useSemiTransparency &&
              (e.target as HTMLElement).hasAttribute("data-tauri-drag-region")) setResizing(true);
        }}>
        <span className="rec-grip" data-tauri-drag-region={drag} title="끌어서 옮기기">
          <IconGripVertical size={16} />
        </span>
        <button
          className={`rec-btn record ${recording ? "on" : ""}`}
          onClick={toggleRecord}
          title={`${recording ? "정지" : "녹화"} (${settings.hotkeyRecord})`}
        >
          {recording ? <IconPlayerStopFilled size={14} /> : <span className="dot" />}
          {recording ? "정지" : "녹화"}
          {!compact && <small>{settings.hotkeyRecord}</small>}
        </button>
        <button
          ref={pauseBtn}
          className={`rec-btn icon ${status?.paused ? "on" : ""}`}
          disabled={!recording}
          onClick={() => api.recPause()}
          title={`일시정지 (${settings.hotkeyPause})`}
        >
          <IconPlayerPause size={18} />
        </button>
        {recording && (
          <button className="rec-btn icon" onClick={saveRecordingImage} title="현재 화면을 PNG로 저장 (Ctrl+S)">
            <IconCamera size={18} />
          </button>
        )}
        {/* 가운데 칸: 자리가 모자라면 여기만 잘려서 오른쪽 닫기 버튼은 늘 보인다 */}
        <div className="rec-mid" data-tauri-drag-region={drag}>
          <span className="rec-timer" data-tauri-drag-region={drag}>
            {formatMs(status?.elapsedMs ?? 0)}
          </span>

          {recording ? (
            <span className="rec-info" data-tauri-drag-region={drag}>
              {status?.frames ?? 0}장 · {status?.memoryMb ?? 0}MB
            </span>
          ) : (
            !compact && (
              <>
                <span className="rec-sep" />
                <label className="rec-dd" title="녹화할 곳 정하기">
                  <IconMarquee2 size={16} />
                  <select
                    value=""
                    onChange={(e) => {
                      const v = e.target.value;
                      if (v === "window") chooseWindow();
                      else if (v === "area") chooseArea();
                      else if (v === "full") fitFullMonitor();
                      else if (v === "half") fitMonitor(2);
                      else if (v === "quarter") fitMonitor(4);
                    }}
                  >
                    <option value="">{fullMonitor ? "전체 화면" : "영역"}</option>
                    <option value="window">창 찾아 맞추기 (Ctrl+Shift+F)</option>
                    <option value="area">영역 드래그 선택 (Ctrl+Shift+D)</option>
                    <option value="full">모니터 전체 화면</option>
                    <option value="half">모니터 절반</option>
                    <option value="quarter">모니터 1/4</option>
                  </select>
                  <IconChevronDown size={14} className="chev" />
                </label>
                {sizeDraft ? (
                  <span className="rec-size" title="녹화 영역 크기 (Enter로 적용, Esc로 취소)">
                    <input
                      autoFocus
                      value={sizeDraft.w}
                      onChange={(e) => setSizeDraft({ ...sizeDraft, w: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") commitSize();
                        else if (e.key === "Escape") setSizeDraft(null);
                      }}
                    />
                    ×
                    <input
                      value={sizeDraft.h}
                      onChange={(e) => setSizeDraft({ ...sizeDraft, h: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") commitSize();
                        else if (e.key === "Escape") setSizeDraft(null);
                      }}
                    />
                    <button className="rec-btn icon small" onClick={commitSize} title="적용">
                      <IconCheck size={14} />
                    </button>
                  </span>
                ) : (
                  <label className="rec-dd" title="녹화 영역 크기">
                    <select
                      value=""
                      onChange={(e) => {
                        const v = e.target.value;
                        if (v === "custom") setSizeDraft({ w: String(captureSize.w), h: String(captureSize.h) });
                        else if (v === "add") {
                          if (!settings.sizePresets.some(([w, h]) => w === captureSize.w && h === captureSize.h))
                            saveSettings({ ...settings, sizePresets: [...settings.sizePresets, [captureSize.w, captureSize.h]] });
                        } else if (v === "remove" && presetValue >= 0) {
                          saveSettings({ ...settings, sizePresets: settings.sizePresets.filter((_, i) => i !== presetValue) });
                        } else if (v !== "") {
                          const [w, h] = settings.sizePresets[Number(v)];
                          setHoleSize(w, h);
                        }
                      }}
                    >
                      <option value="">
                        {captureSize.w} × {captureSize.h}
                      </option>
                      {settings.sizePresets.map(([w, h], i) => (
                        <option key={i} value={i}>
                          {w} × {h}
                        </option>
                      ))}
                      <option value="custom">직접 입력…</option>
                      <option value="add">현재 크기를 목록에 추가</option>
                      {presetValue >= 0 && <option value="remove">현재 크기를 목록에서 삭제</option>}
                    </select>
                    <IconChevronDown size={14} className="chev" />
                  </label>
                )}
                {fpsDraft !== null ? (
                  <span className="rec-size" title="FPS (1~100), Enter로 적용, Esc로 취소">
                    <input
                      autoFocus
                      value={fpsDraft}
                      onChange={(e) => setFpsDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Escape") setFpsDraft(null);
                        if (e.key !== "Enter") return;
                        const fps = Math.round(Number(fpsDraft));
                        setFpsDraft(null);
                        if (fps >= 1 && fps <= 100) saveSettings({ ...settings, fps });
                        else showNotice("FPS는 1~100 사이로 입력해 주세요");
                      }}
                      onBlur={() => setFpsDraft(null)}
                    />
                    FPS
                  </span>
                ) : (
                  <label className="rec-dd" title="초당 프레임 수">
                    <select
                      value={FPS_LIST.includes(settings.fps) ? settings.fps : "custom"}
                      onChange={(e) => {
                        if (e.target.value === "custom") return setFpsDraft(String(settings.fps));
                        saveSettings({ ...settings, fps: Math.round(Number(e.target.value)) });
                      }}
                    >
                      {FPS_LIST.map((f) => (
                        <option key={f} value={f}>
                          {f} FPS
                        </option>
                      ))}
                      <option value="custom">{FPS_LIST.includes(settings.fps) ? "직접 입력" : `${settings.fps} FPS`}</option>
                    </select>
                    <IconChevronDown size={14} className="chev" />
                  </label>
                )}
                {wide && (
                  <>
                    <span className="rec-sep" />
                    <label className="rec-switch" title="마우스 커서 녹화">
                      <IconPointer size={16} />
                      커서
                      <input
                        type="checkbox"
                        checked={settings.captureCursor}
                        onChange={(e) => saveSettings({ ...settings, captureCursor: e.target.checked })}
                      />
                      <i />
                    </label>
                    <label className="rec-switch" title="녹화하면서 MP4 파일로 바로 저장">
                      <IconMovie size={16} />
                      MP4 바로 저장
                      <input
                        type="checkbox"
                        checked={settings.recDirectSave}
                        onChange={(e) => saveSettings({ ...settings, recDirectSave: e.target.checked })}
                      />
                      <i />
                    </label>
                  </>
                )}
              </>
            )
          )}

        </div>
        <span className="rec-spacer" data-tauri-drag-region={drag} />
        {windowChoices && !recording && (
          <select
            className="rec-select"
            autoFocus
            value=""
            aria-label="녹화할 창 선택"
            onBlur={() => setWindowChoices(null)}
            onKeyDown={(e) => { if (e.key === "Escape") setWindowChoices(null); }}
            onChange={(e) => {
              const target = windowChoices[Number(e.target.value)];
              setWindowChoices(null);
              if (target) void fitWindow(target);
            }}
          >
            <option value="">창 선택…</option>
            {windowChoices.map((c, i) => <option key={`${c.id}-${c.pid}`} value={i}>{c.title}</option>)}
          </select>
        )}
        {wide && (
          <>
            <button className="rec-btn icon ghost" disabled={recording} onClick={openFiles} title="파일 열기 (GIF·동영상·이미지)">
              <IconFolderOpen size={18} />
            </button>
            <button className="rec-btn icon ghost" disabled={recording} onClick={openSettings} title="설정">
              <IconSettings size={18} />
            </button>
          </>
        )}
        <label className={`rec-btn icon ghost menu ${recording ? "disabled" : ""}`} title="메뉴">
          <IconMenu2 size={18} />
          <select
            value=""
            disabled={recording}
            onChange={(e) => {
              const v = e.target.value;
              if (v === "open") openFiles();
              else if (v === "window") chooseWindow();
              else if (v === "area") chooseArea();
              else if (v === "full") fitFullMonitor();
              else if (v === "settings") openSettings();
              else if (v === "mp4") saveSettings({ ...settings, recDirectSave: !settings.recDirectSave });
              else if (v === "cursor") saveSettings({ ...settings, captureCursor: !settings.captureCursor });
              else if (v) openMenu(v);
            }}
          >
            <option value="">메뉴</option>
            <option value="open">파일 열기…</option>
            <option value="window">창 찾아 맞추기 (Ctrl+Shift+F)</option>
            <option value="area">영역 드래그 선택 (Ctrl+Shift+D)</option>
            <option value="full">모니터 전체 화면</option>
            <option value="newBlank">빈 이미지/영상 만들기…</option>
            <option value="collage">콜라주…</option>
            <option value="manage">파일 관리…</option>
            <option value="mp4">MP4 바로 저장: {settings.recDirectSave ? "켜짐 → 끄기" : "꺼짐 → 켜기"}</option>
            <option value="cursor">커서 녹화: {settings.captureCursor ? "켜짐 → 끄기" : "꺼짐 → 켜기"}</option>
            <option value="settings">설정…</option>
          </select>
        </label>
        <button className="rec-btn icon ghost" onClick={() => win.minimize()} title="최소화">
          <IconMinus size={18} />
        </button>
        <button className="rec-btn icon ghost close" disabled={recording} onClick={() => win.close()} title="닫기 (편집 창으로 돌아갑니다)">
          <IconX size={18} />
        </button>
        {/* 틀이 좁으면 툴바 가운데에 자리가 없어서, 알림은 녹화·일시정지 버튼 오른쪽을 덮어 띄운다. 누르면 닫힌다. */}
        {notice && (
          <div
            className="rec-notice"
            title={notice}
            onClick={() => setNotice(null)}
            style={{ left: pauseBtn.current ? pauseBtn.current.offsetLeft + pauseBtn.current.offsetWidth + 6 : 84 }}
          >
            {notice}
          </div>
        )}
      </div>

      <div className="rec-frame" style={frameStyle}>
        {/* 테두리를 잡고 크기를 바꾼다 */}
        <div className="grip n" onMouseDown={grip("North")} />
        <div className="grip s" onMouseDown={grip("South")} />
        <div className="grip e" onMouseDown={grip("East")} />
        <div className="grip w" onMouseDown={grip("West")} />
        <div className="grip ne" onMouseDown={grip("NorthEast")} />
        <div className="grip nw" onMouseDown={grip("NorthWest")} />
        <div className="grip se" onMouseDown={grip("SouthEast")} />
        <div className="grip sw" onMouseDown={grip("SouthWest")} />
      </div>
    </div>
  );
}
