// 편집 창: 미리보기 + 프레임 목록 + 편집 도구 + 저장.

import { ComponentType, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, EditOp, errText, extOf, formatBytes, formatMs, frameUrl, hexToRgb, ProjectView, Settings, VIDEO_EXTS, VideoInfo } from "./api";
import { beginAppend, blocksProjectChange, createAppendGuard, finishAppend, invalidateAppend, isCurrentAppend } from "./appendGuard";
import {
  IconAdjustmentsHorizontal,
  IconArrowBackUp,
  IconArrowForwardUp,
  IconArrowsLeftRight,
  IconArrowsMaximize,
  IconCircleDot,
  IconClipboard,
  IconClock,
  IconCopy,
  IconCrop,
  IconCut,
  IconDeviceFloppy,
  IconDimensions,
  IconFolderOpen,
  IconFolders,
  IconGauge,
  IconHistory,
  IconLayoutGrid,
  IconMinus,
  IconPencil,
  IconPhotoDown,
  IconPlayerPause,
  IconPlayerPlay,
  IconPlayerSkipBack,
  IconPlayerSkipForward,
  IconPlus,
  IconRepeat,
  IconScissors,
  IconSettings,
  IconStack2,
  IconStackPop,
  IconTransitionRight,
  IconTrash,
  IconTrashX,
  IconTypography,
  IconZoomInArea,
} from "@tabler/icons-react";
import {
  isModalOpen,
  CutRangeDialog,
  DelayDialog,
  ReduceDialog,
  ResizeDialog,
  SummaryDialog,
  TransitionDialog,
  UpscaleDialog,
} from "./dialogs";
import SaveDialog from "./SaveDialog";
import SettingsDialog from "./SettingsDialog";
import CropView from "./CropView";
import DrawView from "./DrawView";
import FilterView from "./FilterView";
import InsertView from "./InsertView";
import ZoomView from "./ZoomView";
import RecoveryList from "./RecoveryList";
import { ImagesImportDialog, VideoImportDialog } from "./ImportDialogs";
import { CollageDialog, ExportFramesDialog, ManageDialog, NewBlankDialog } from "./ToolsDialogs";
import FrameStrip from "./FrameStrip";
import "./editor.css";
import { alertAsk, AskHost, confirmAsk } from "./ask";
import { sortByFileName } from "./fileOrder";
import { SidePanelTarget } from "./SidePanel";
import { ModalInPanel } from "./dialogs";
import { DirtyReport, FrameContext } from "./Panel";
import { checkForUpdate } from "./updater";

type Dialog = null | "reduce" | "resize" | "delay" | "cut" | "save" | "settings" | "transition" | "summary" | "upscale" | "newBlank" | "collage" | "manage" | "exportFrames";
/** 미리보기 자리를 차지하는 도구 화면 */
type Mode = null | "crop" | "filter" | "draw" | "insert" | "zoom";
type Icon = ComponentType<{ size?: number; stroke?: number }>;

/** 왼쪽 도구 목록: [id, 아이콘, 이름, 단축키]. id가 Mode면 도구 화면을, 아니면 대화상자를 연다. */
const RAIL: [Exclude<Mode | Dialog, null>, Icon, string, string][] = [
  ["reduce", IconStackPop, "프레임", "Q"],
  ["crop", IconCrop, "자르기", "C"],
  ["resize", IconDimensions, "크기 · 회전", "R"],
  ["filter", IconAdjustmentsHorizontal, "필터", "F"],
  ["draw", IconPencil, "그리기", "D"],
  ["insert", IconTypography, "텍스트 · 이미지", "B"],
  ["transition", IconTransitionRight, "화면 전환", "T"],
  ["zoom", IconZoomInArea, "줌", ""],
  ["summary", IconLayoutGrid, "영상 요약", "S"],
  ["upscale", IconArrowsMaximize, "고품질 확대", ""],
];
const MODES: string[] = ["crop", "filter", "draw", "insert", "zoom"];
/** 대화상자 대신 오른쪽 패널에 설정을 띄우는 도구 */
const PANEL_DIALOGS: string[] = ["reduce", "resize", "transition", "summary", "upscale"];
/** 열기에서 옵션을 물어봐야 하는 경우 */
type ImportRequest = null | { kind: "video"; path: string; info: VideoInfo } | { kind: "images"; paths: string[] };

/** 파일을 끌어 오는 동안: 놓으면 맨 뒤에 붙는지, 새로 여는지, 받지 않는지(null) */
type DropKind = null | "append" | "open";

/** 움직이는 그림일 수 있는 확장자 (하나만 열면 옵션 없이 바로 불러온다) */
const ANIMATED_EXTS = ["gif", "webp", "png", "apng"];

/** 프레임 작업 (오른쪽 패널 버튼과 썸네일 오른쪽 클릭 메뉴): [값, 이름, 아이콘, 단축키] */
const FRAME_ACTIONS: [string, string, Icon, string][] = [
  ["copy", "복사", IconCopy, "Ctrl+C"],
  ["cut", "잘라내기", IconCut, "Ctrl+X"],
  ["paste", "붙여넣기", IconClipboard, "Ctrl+V"],
  ["saveImage", "이미지로 저장", IconPhotoDown, ""],
];

/** 확대 배율 한계 */
const ZOOM_MIN = 0.05;
const ZOOM_MAX = 8;

const EMPTY: ProjectView = { frames: [], canUndo: false, canRedo: false, dirty: false, selection: null, message: null, memoryBytes: 0 };

export default function Editor() {
  const [view, setView] = useState<ProjectView>(EMPTY);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [sel, setSel] = useState<number[]>([]);
  const [anchor, setAnchor] = useState(0);
  const [current, setCurrent] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [zoom, setZoom] = useState<number | "fit">("fit");
  const [dialog, setDialog] = useState<Dialog>(null);
  const [mode, setModeRaw] = useState<Mode>(null);
  const [busy, setBusy] = useState(false);
  const appendGuard = useRef(createAppendGuard());
  const [busyText, setBusyText] = useState("처리 중…");
  const [importReq, setImportReq] = useState<ImportRequest>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  /**
   * 저장한 뒤 그 파일을 저장 창에서 바로 지웠나. Rust 쪽은 저장했을 때 "저장됨"으로 바꾼 뒤 그대로라서,
   * 다른 작업을 열거나 다시 저장할 때까지 화면에서 기억해 두고 버릴 때 묻는다.
   */
  const [savedFileDeleted, setSavedFileDeleted] = useState(false);
  /** 저장하기 직전에 저장 안 한 내용이 있었나 (저장한 파일을 지우면 그 상태로 돌아간다) */
  const dirtyBeforeSave = useRef(false);
  /** 녹화 틀의 열기·☰ 메뉴·설정에서 넘어왔나. 그랬다면 취소할 때 녹화 틀로 돌아간다. */
  const fromRecorder = useRef(false);
  const [stage, setStage] = useState({ w: 800, h: 500 });
  /** 오른쪽 패널 자리. 도구 화면이 설정 패널을 여기로 옮겨 그린다. */
  const [sideEl, setSideEl] = useState<HTMLElement | null>(null);
  /** 열린 도구 패널에 아직 적용하지 않은 변경이 있나 (패널이 알려 준다) */
  const toolDirty = useRef(false);
  const reportDirty = useCallback((d: boolean) => {
    toolDirty.current = d;
  }, []);

  const frames = view.frames;
  const cur = frames[Math.min(current, frames.length - 1)];
  const total = useMemo(() => frames.reduce((a, f) => a + f.delayMs, 0), [frames]);
  const curStart = useMemo(() => frames.slice(0, current).reduce((a, f) => a + f.delayMs, 0), [frames, current]);

  const showToast = useCallback((m: string) => {
    setToast(m);
    window.setTimeout(() => setToast((c) => (c === m ? null : c)), 4000);
  }, []);

  const blockWhileAppending = useCallback(() => {
    if (!blocksProjectChange(appendGuard.current)) return false;
    showToast("파일을 붙이는 중입니다. 끝난 뒤 다시 해 주세요");
    return true;
  }, [showToast]);

  const openNewDialog = useCallback((d: Dialog) => {
    if ((d === "newBlank" || d === "collage") && blockWhileAppending()) return;
    setDialog(d);
  }, [blockWhileAppending]);

  const load = useCallback(async () => {
    appendGuard.current = invalidateAppend(appendGuard.current);
    fromRecorder.current = false;
    const v = await api.projectView();
    setView(v);
    setSavedFileDeleted(false);
    setSel([]);
    setCurrent(0);
    setPlaying(false);
    setModeRaw(null);
  }, []);

  useEffect(() => {
    api.getSettings().then((s) => {
      setSettings(s);
      if (s.checkUpdates && !import.meta.env.DEV) checkForUpdate();
    });
    load();
    // 프로그램을 켜면 편집 창이 먼저 뜨므로, 시작할 때 생긴 문제(단축키 등록 실패 등)는 여기서 알린다
    api.takeStartupNotice().then((m) => m && showToast(m));
    const un = [
      listen("project-changed", () => load()),
      listen("settings-changed", () => api.getSettings().then(setSettings)),
      listen<string>("notice", (e) => showToast(e.payload)),
      listen("open-settings", () => {
        fromRecorder.current = true;
        setDialog("settings");
      }),
      listen<Dialog>("open-dialog", (e) => {
        if (blockWhileAppending()) return;
        fromRecorder.current = true;
        openNewDialog(e.payload);
      }),
      // 녹화가 끝나면 바로 저장 창 (설정에서 "바로 편집"을 켜면 편집만)
      listen("recorded", async () => {
        const s = await api.getSettings();
        if (!s.gotoEditAfterRecording) setDialog("save");
      }),
      listen<{ progress: number; message: string }>("import-progress", (e) =>
        setBusyText(`${e.payload.message} ${Math.round(e.payload.progress * 100)}%`),
      ),
    ];
    return () => un.forEach((p) => p.then((f) => f()));
  }, [load, showToast, blockWhileAppending, openNewDialog]);

  // 미리보기 자리의 크기를 잰다 ("맞춤" 배율의 기준). 자리가 새로 그려져도 다시 붙고,
  // 창이 숨겨지거나 최소화되어 0이 될 때는 무시한다 (그 값이 남으면 맞춤이 5%가 됐다).
  // Ctrl+휠: 마우스가 가리키는 곳을 그대로 두고 확대·축소한다. 휠이 지금 배율을 알아야 해서 ref로 들고 있다.
  const scaleNow = useRef(1);
  const zoomAnchor = useRef<{ x: number; y: number; fx: number; fy: number } | null>(null);
  const stageCleanup = useRef<(() => void) | null>(null);
  const stageEl = useRef<HTMLElement | null>(null);
  const stageRef = useCallback((el: HTMLElement | null) => {
    stageCleanup.current?.();
    stageCleanup.current = null;
    stageEl.current = el;
    if (!el) return;
    const measure = () => {
      if (el.isConnected && el.clientWidth > 0 && el.clientHeight > 0) setStage({ w: el.clientWidth, h: el.clientHeight });
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    // 웹뷰 기본 동작(페이지 확대)을 막아야 해서 passive가 아닌 리스너를 직접 붙인다
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      const content = el.querySelector<HTMLElement>(".ed-canvas, .tool-box, .crop-box");
      if (!content) return;
      e.preventDefault();
      const r = content.getBoundingClientRect();
      zoomAnchor.current = { x: e.clientX, y: e.clientY, fx: (e.clientX - r.left) / r.width, fy: (e.clientY - r.top) / r.height };
      const next = scaleNow.current * (e.deltaY < 0 ? 1.1 : 1 / 1.1);
      setZoom(Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, next)));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    stageCleanup.current = () => {
      ro.disconnect();
      el.removeEventListener("wheel", onWheel);
    };
  }, []);

  /** 저장하지 않은 녹화·편집 내용을 버리기 전에 물어야 하나 (설정에서 끌 수 있다) */
  const askDiscard = view.frames.length > 0 && (view.dirty || savedFileDeleted) && !settings?.doNotAskDiscardRecording;

  /** 녹화 틀에서 넘어왔다가 아무것도 불러오지 않고 끝냈으면 녹화 틀로 돌아간다 */
  const returnToRecorderIfCancelled = () => {
    if (blockWhileAppending()) return;
    if (!fromRecorder.current) return;
    fromRecorder.current = false;
    if (view.frames.length === 0) api.showRecorder();
  };

  /** 대화상자 닫기. 녹화 틀에서 열었던 경우(프레임 없음)에는 녹화 틀로 돌아간다. */
  const closeDialog = () => {
    setDialog(null);
    returnToRecorderIfCancelled();
  };

  /** 편집 창을 닫고 녹화 틀로 (저장 안 한 내용이 있으면 묻는다) */
  const backToRecorder = useCallback(async () => {
    if (blockWhileAppending()) return;
    if (
      askDiscard &&
      !(await confirmAsk("저장하지 않은 내용이 있습니다. 버리고 녹화 화면으로 돌아갈까요?", "버리고 돌아가기"))
    )
      return;
    if (blockWhileAppending()) return;
    setView(EMPTY);
    setDialog(null);
    await api.showRecorder();
  }, [view, askDiscard, blockWhileAppending]);

  useEffect(() => {
    const un = listen("close-requested", async () => {
      // 녹화 틀에서 설정만 열었던 경우
      if (fromRecorder.current && view.frames.length === 0) {
        setDialog(null);
        returnToRecorderIfCancelled();
        return;
      }
      if (
        askDiscard &&
        !(await confirmAsk("저장하지 않은 내용이 있습니다. 버리고 끝낼까요?", "버리고 끝내기"))
      )
        return;
      await api.quitApp();
    });
    return () => {
      un.then((f) => f());
    };
  }, [view, askDiscard]);

  const applyView = useCallback((v: ProjectView) => {
    setView(v);
    const n = v.frames.length;
    if (n > 0) fromRecorder.current = false;
    if (v.selection) setSel(v.selection);
    else setSel((s) => s.filter((i) => i < n));
    setCurrent((c) => Math.min(v.selection?.[0] ?? c, Math.max(0, n - 1)));
    if (v.message) showToast(v.message);
  }, [showToast]);

  /** 편집을 실행하고 결과로 화면을 갱신한다 */
  const exec = useCallback(
    async (task: () => Promise<ProjectView>) => {
      if (blockWhileAppending()) return;
      setBusy(true);
      setPlaying(false);
      try {
        applyView(await task());
      } catch (e) {
        showToast(errText(e));
      } finally {
        setBusy(false);
        setBusyText("처리 중…");
      }
    },
    [showToast, applyView, blockWhileAppending],
  );

  /** 파일 열기: 동영상이면 구간을, 이미지 여러 장이면 옵션을 묻고, 움직이는 그림 하나면 바로 불러온다 */
  const openPaths = useCallback(
    async (picked?: string[] | null) => {
      if (blockWhileAppending()) return;
      const paths = picked ?? (await api.pickOpenFiles());
      if (blockWhileAppending()) return;
      if (!paths || paths.length === 0) {
        // 녹화 틀에서 열기를 눌렀다가 취소한 경우
        returnToRecorderIfCancelled();
        return;
      }
      if (
        askDiscard &&
        !(await confirmAsk("저장하지 않은 내용이 있습니다. 버리고 새 파일을 열까요?", "버리고 열기"))
      )
        return;
      if (blockWhileAppending()) return;
      const sorted = sortByFileName(paths);
      const videos = sorted.filter((p) => VIDEO_EXTS.includes(extOf(p)));
      if (videos.length > 0) {
        if (sorted.length > 1) return showToast("동영상은 하나씩 따로 열어 주세요");
        setBusy(true);
        try {
          setImportReq({ kind: "video", path: videos[0], info: await api.probeVideo(videos[0]) });
        } catch (e) {
          showToast(errText(e));
        } finally {
          setBusy(false);
        }
      } else if (sorted.length === 1 && ANIMATED_EXTS.includes(extOf(sorted[0]))) {
        await exec(() => api.importFiles(sorted, 100, "contain"));
      } else {
        setImportReq({ kind: "images", paths: sorted });
      }
    },
    [view, askDiscard, exec, showToast, blockWhileAppending],
  );

  /** 끌어 놓은 파일을 편집 중인 프레임 맨 뒤에 붙인다 (파일 이름 순서, 되돌리기 한 번). 못 읽은 파일은 끝나고 한 번에 알린다. */
  const appendPaths = useCallback(
    async (paths: string[]) => {
      const started = beginAppend(appendGuard.current);
      if (!started) {
        blockWhileAppending();
        return;
      }
      // 첫 await 전에 잠가서 같은 순간에 들어오는 두 번째 drop도 막는다.
      appendGuard.current = started.guard;
      const job = started.job;
      setBusy(true);
      setPlaying(false);
      try {
        const sorted = sortByFileName(paths);
        const r = await api.appendFiles(sorted);
        if (!isCurrentAppend(appendGuard.current, job)) {
          // 화면 작업 번호가 낡아도 Rust는 붙였을 수 있으므로 지금 내용을 다시 읽는다.
          const v = await api.projectView();
          setView(v);
          const n = v.frames.length;
          setSel((s) => s.filter((i) => i >= 0 && i < n));
          setCurrent((c) => Math.max(0, Math.min(c, n - 1)));
          return;
        }
        applyView(r.view);
        const skipped = r.skipped;
        if (skipped.length === 0) return;
        const shown = skipped.slice(0, 15).map((s) => `· ${s.name}: ${s.reason}`);
        if (skipped.length > shown.length) shown.push(`… 그 밖에 ${skipped.length - shown.length}개`);
        const head =
          skipped.length === sorted.length
            ? "파일을 붙이지 못했습니다. 편집 중인 프레임은 그대로입니다."
            : `${skipped.length}개 파일은 붙이지 못하고 건너뛰었습니다.`;
        await alertAsk(`${head}\n\n${shown.join("\n")}`);
      } catch (e) {
        if (isCurrentAppend(appendGuard.current, job)) showToast(errText(e));
      } finally {
        if (appendGuard.current.activeId === job.id) {
          appendGuard.current = finishAppend(appendGuard.current, job);
          setBusy(false);
          setBusyText("처리 중…");
        }
      }
    },
    [applyView, showToast, blockWhileAppending],
  );

  // 녹화 틀의 열기 버튼
  useEffect(() => {
    const un = listen("open-files", () => {
      if (blockWhileAppending()) return;
      fromRecorder.current = true;
      openPaths();
    });
    return () => {
      un.then((f) => f());
    };
  }, [openPaths, blockWhileAppending]);

  // 파일 끌어 놓기: 편집 중이면 맨 뒤에 붙이고, 아니면 연다.
  // 처리 중이거나 창·도구 화면이 떠 있으면 단축키처럼 받지 않는다. 한 번만 등록하고 지금 상태는 ref로 본다.
  const [dropHint, setDropHint] = useState<DropKind>(null);
  const drop = useRef<{ kind: () => DropKind; run: (paths: string[]) => void }>({ kind: () => null, run: () => {} });
  drop.current = {
    kind: () => (blocksProjectChange(appendGuard.current) || busy || dialog || importReq || menu || mode || isModalOpen() ? null : frames.length > 0 ? "append" : "open"),
    run: (paths) => (frames.length > 0 ? appendPaths(paths) : openPaths(paths)),
  };
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") return setDropHint(drop.current.kind());
      setDropHint(null);
      if (p.type !== "drop" || p.paths.length === 0) return;
      if (drop.current.kind()) drop.current.run(p.paths);
      else showToast("처리 중이거나 창·도구가 열려 있어 파일을 받지 않았습니다");
    });
    return () => {
      un.then((f) => f());
    };
  }, [showToast]);
  const run = useCallback((op: EditOp) => exec(() => api.edit(op)), [exec]);

  const setMode = (m: Mode) => {
    setPlaying(false);
    setModeRaw((cur) => (cur === m ? null : m));
  };

  // 재생: 프레임마다 자기 시간만큼 보여준다
  useEffect(() => {
    if (!playing || frames.length < 2) return;
    const f = frames[current];
    // 다음 몇 장을 미리 불러 둔다
    for (let k = 1; k <= 3; k++) new Image().src = frameUrl(frames[(current + k) % frames.length].id);
    const t = window.setTimeout(() => setCurrent((c) => (c + 1) % frames.length), Math.max(10, f?.delayMs ?? 100));
    return () => window.clearTimeout(t);
  }, [playing, current, frames]);

  const selectFrame = useCallback(
    (i: number, e: { ctrlKey: boolean; shiftKey: boolean }) => {
      setPlaying(false);
      setCurrent(i);
      if (e.shiftKey) {
        const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
        setSel(Array.from({ length: b - a + 1 }, (_, k) => a + k));
      } else if (e.ctrlKey) {
        setSel((s) => (s.includes(i) ? s.filter((x) => x !== i) : [...s, i].sort((x, y) => x - y)));
        setAnchor(i);
      } else {
        setSel([i]);
        setAnchor(i);
      }
    },
    [anchor],
  );

  /** 선택이 없으면 지금 보고 있는 프레임 */
  const target = sel.length > 0 ? sel : frames.length ? [current] : [];

  /** 프레임 작업 (오른쪽 패널 버튼과 썸네일 오른쪽 클릭 메뉴가 함께 쓴다) */
  const frameAction = (v: string) => {
    if (v === "delete") run({ op: "delete", sel: target });
    else if (v === "copy")
      // 편집 안에서 붙여넣을 프레임으로 기억하고, 지금 프레임은 다른 프로그램에 붙여넣게 클립보드에도
      run({ op: "copy", sel: target })
        .then(() => api.copyFrameImage(current))
        .then(() => showToast(`${target.length}장 복사 (지금 프레임은 클립보드에도 복사되었습니다)`))
        .catch((e) => showToast(errText(e)));
    else if (v === "cut") run({ op: "cut", sel: target });
    else if (v === "paste") run({ op: "paste", at: current + 1 });
    else if (v === "saveImage" && sel.length > 1) setDialog("exportFrames");
    else if (v === "saveImage")
      (async () => {
        const name = `frame_${String(current + 1).padStart(4, "0")}.png`;
        const path = await api.pickSaveFile(`${settings?.outputDir ?? ""}\\${name}`, [
          ["PNG", ["png"]],
          ["JPG", ["jpg"]],
          ["WebP", ["webp"]],
          ["BMP", ["bmp"]],
        ]);
        if (!path) return;
        await api.saveFrameImage(current, path);
        showToast("이미지로 저장했습니다");
      })().catch((e) => showToast(errText(e)));
  };

  /** 썸네일 끌어 놓기: 잡은 프레임이 선택에 들어 있으면 선택한 프레임을 함께 옮긴다 */
  const moveFrames = (i: number, to: number) => !busy && run({ op: "moveTo", sel: sel.includes(i) ? sel : [i], to });

  // 단축키
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
      // 처리 중에는 버튼처럼 단축키도 막는다 (편집이 겹치지 않게)
      if (blocksProjectChange(appendGuard.current)) {
        const ctrl = e.ctrlKey || e.metaKey;
        const k = e.key.toLowerCase();
        if ((ctrl && k === "o") || (!ctrl && k === "g")) {
          e.preventDefault();
          blockWhileAppending();
        }
        return;
      }
      if (busy || dialog || menu || frames.length === 0 || isModalOpen()) return;
      if (mode) return; // 도구 화면이 자기 단축키를 처리한다
      const k = e.key.toLowerCase();
      const ctrl = e.ctrlKey || e.metaKey;
      let handled = true;
      if (k === " ") setPlaying((p) => !p);
      else if (k === "delete") run({ op: "delete", sel: target });
      else if (ctrl && k === "a") setSel(frames.map((_, i) => i));
      else if (ctrl && k === "c") frameAction("copy");
      else if (ctrl && e.shiftKey && k === "e") setDialog("exportFrames");
      else if (ctrl && k === "x") run({ op: "cut", sel: target });
      else if (ctrl && k === "v") run({ op: "paste", at: current + 1 });
      else if (ctrl && (k === "y" || (e.shiftKey && k === "z"))) run({ op: "redo" });
      else if (ctrl && k === "z") run({ op: "undo" });
      else if (ctrl && k === "s") setDialog("save");
      else if (ctrl && k === "o") openPaths();
      else if (e.altKey && k === "arrowleft") run({ op: "move", sel: target, dir: -1 });
      else if (e.altKey && k === "arrowright") run({ op: "move", sel: target, dir: 1 });
      else if (k === "arrowleft" || k === "arrowright") {
        const i = Math.max(0, Math.min(frames.length - 1, current + (k === "arrowleft" ? -1 : 1)));
        selectFrame(i, { ctrlKey: false, shiftKey: e.shiftKey });
      } else if (k === "home") selectFrame(0, { ctrlKey: false, shiftKey: e.shiftKey });
      else if (k === "end") selectFrame(frames.length - 1, { ctrlKey: false, shiftKey: e.shiftKey });
      else if (!ctrl && k === "q") setDialog("reduce");
      else if (!ctrl && k === "c") setMode("crop");
      else if (!ctrl && k === "f") setMode("filter");
      else if (!ctrl && k === "d") setMode("draw");
      else if (!ctrl && k === "b") setMode("insert");
      else if (!ctrl && k === "t") setDialog("transition");
      else if (!ctrl && k === "s") setDialog("summary");
      else if (!ctrl && k === "g") openNewDialog("collage");
      else if (!ctrl && k === "r") setDialog("resize");
      else if (!ctrl && k === "x") setDialog("cut");
      else if (k === "escape") setSel([]);
      else handled = false;
      if (handled) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const scale = cur
    ? zoom === "fit"
      ? Math.max(ZOOM_MIN, Math.min(1, (stage.w - 24) / cur.width, (stage.h - 24) / cur.height))
      : zoom
    : 1;
  scaleNow.current = scale;
  // 크기 감시는 화면을 그릴 때만 알려 주므로(숨은 창 등) 화면이 바뀔 때마다·창 크기가 바뀔 때도 직접 잰다.
  // 오른쪽 패널이 열리고 닫혀 자리가 바뀐 경우도 여기서 잡힌다.
  const remeasure = useCallback(() => {
    const el = stageEl.current;
    if (!el || el.clientWidth <= 0 || el.clientHeight <= 0) return;
    setStage((s) => (s.w === el.clientWidth && s.h === el.clientHeight ? s : { w: el.clientWidth, h: el.clientHeight }));
  }, []);
  useLayoutEffect(remeasure);
  useEffect(() => {
    window.addEventListener("resize", remeasure);
    document.addEventListener("visibilitychange", remeasure);
    return () => {
      window.removeEventListener("resize", remeasure);
      document.removeEventListener("visibilitychange", remeasure);
    };
  }, [remeasure]);
  // 배율이 바뀐 뒤, Ctrl+휠로 잡아 둔 지점이 다시 마우스 아래에 오도록 스크롤을 옮긴다
  // (그림을 담고 스크롤되는 칸은 그림 바로 바깥: 편집 화면은 미리보기 자리, 도구 화면은 도구 칸)
  useLayoutEffect(() => {
    const a = zoomAnchor.current;
    zoomAnchor.current = null;
    const content = stageEl.current?.querySelector<HTMLElement>(".ed-canvas, .tool-box, .crop-box");
    const area = content?.parentElement;
    if (!a || !content || !area) return;
    const r = content.getBoundingClientRect();
    area.scrollLeft += r.left + a.fx * r.width - a.x;
    area.scrollTop += r.top + a.fy * r.height - a.y;
  }, [scale]);


  const empty = frames.length === 0;

  /** 열린 도구에 적용하지 않은 변경이 있으면 버려도 되는지 묻는다 */
  const leaveTool = async () => {
    if (!toolDirty.current) return true;
    if (!(await confirmAsk("적용하지 않은 변경이 있습니다. 버리고 다른 도구로 바꿀까요?", "버리고 바꾸기"))) return false;
    toolDirty.current = false;
    return true;
  };

  /** 왼쪽 도구 목록 항목 열기 (같은 것을 다시 누르면 닫힌다) */
  const openTool = async (id: Exclude<Mode | Dialog, null>) => {
    if (!(await leaveTool())) return;
    if (MODES.includes(id)) {
      setDialog(null);
      setMode(id as Mode);
    } else {
      setModeRaw(null);
      setDialog((d) => (d === id ? null : (id as Dialog)));
    }
  };
  const activeTool: string | null = mode ?? dialog;
  /** 오른쪽 패널은 왼쪽 도구를 골랐을 때만 (그 도구의 세부 설정) */
  const showSide = !empty && (mode !== null || (dialog !== null && PANEL_DIALOGS.includes(dialog)));
  const frameCtx = useMemo(() => ({ current, total: frames.length, sel }), [current, frames.length, sel]);

  return (
    <SidePanelTarget.Provider value={sideEl}>
    <FrameContext.Provider value={frameCtx}>
    <DirtyReport.Provider value={reportDirty}>
    <div className="ed">
      <AskHost />
      {dropHint && (
        <div className="ed-drop">
          <span>{dropHint === "append" ? "놓으면 맨 뒤에 추가" : "놓으면 열기"}</span>
        </div>
      )}
      <header className="ed-top">
        <button className="rec" disabled={busy} onClick={backToRecorder} title="녹화 틀을 띄워 새로 녹화">
          <IconCircleDot size={18} />새 녹화
        </button>
        <button disabled={busy} onClick={() => openPaths()} title="GIF·WebP·APNG·동영상·이미지 열기 (Ctrl+O). 편집 중에 파일을 끌어 놓으면 맨 뒤에 붙습니다.">
          <IconFolderOpen size={18} />열기
        </button>
        <select
          disabled={busy}
          value=""
          onChange={(e) => {
            const v = e.target.value as Dialog;
            if (v) openNewDialog(v);
          }}
          title="새로 만들기"
        >
          <option value="">새로 만들기</option>
          <option value="newBlank">빈 이미지/영상…</option>
          <option value="collage">콜라주… (G)</option>
        </select>
        <button className="ghost" onClick={() => setDialog("manage")} title="저장 폴더의 파일 관리">
          <IconFolders size={18} />
          <span className="lbl-wide">파일 관리</span>
        </button>
        <span className="sep" />
        <button className="ghost icon" disabled={empty || !view.canUndo || busy} onClick={() => run({ op: "undo" })} title="되돌리기 (Ctrl+Z)">
          <IconArrowBackUp size={18} />
        </button>
        <button className="ghost icon" disabled={empty || !view.canRedo || busy} onClick={() => run({ op: "redo" })} title="다시 실행 (Ctrl+Y)">
          <IconArrowForwardUp size={18} />
        </button>
        <button
          className="ghost icon"
          disabled={empty || busy}
          onClick={async () => (await confirmAsk("처음 상태로 되돌릴까요?")) && run({ op: "restoreInitial" })}
          title="처음 상태로"
        >
          <IconHistory size={18} />
        </button>
        <span className="sep" />
        {FRAME_ACTIONS.map(([v, label, Icon, key]) => (
          <button
            key={v}
            className="ghost"
            disabled={empty || busy}
            onClick={() => frameAction(v)}
            title={v === "saveImage" && sel.length > 1 ? "선택한 프레임을 이미지로 내보내기 (Ctrl+Shift+E)" : key ? `${label} (${key})` : label}
          >
            <Icon size={18} />
            <span className="lbl">{label}</span>
          </button>
        ))}
        <span className="grow" />
        <button className="primary" disabled={empty || busy} onClick={() => setDialog("save")} title="저장 (Ctrl+S)">
          <IconDeviceFloppy size={18} />저장
        </button>
      </header>

      <div className={`ed-body ${showSide ? "" : "no-side"}`}>
        <nav className="ed-rail">
          {RAIL.map(([id, Icon, label, key]) => (
            <button
              key={id}
              className={activeTool === id ? "on" : ""}
              disabled={empty || busy}
              onClick={() => openTool(id)}
              title={key ? `${label} (${key})` : label}
            >
              <Icon size={20} />
              <span>{label}</span>
              {key && <kbd>{key}</kbd>}
            </button>
          ))}
          <span className="grow" />
          <button
            className={dialog === "settings" ? "on" : ""}
            onClick={async () => (dialog && PANEL_DIALOGS.includes(dialog) ? await leaveTool() : true) && setDialog("settings")}
            title="설정"
          >
            <IconSettings size={20} />
            <span>설정</span>
          </button>
        </nav>

        <section className="ed-center">
          {!empty && cur && (
            <div className="ed-info">
              <b>
                {cur.width} × {cur.height}
              </b>
              <span>·</span>
              <span>{frames.length} 프레임</span>
              <span>·</span>
              <span>{formatMs(total)}</span>
              <span>·</span>
              <span title="편집에 쓰는 메모리 (되돌리기·다시 실행 기록 포함)">메모리 {formatBytes(view.memoryBytes)}</span>
            </div>
          )}
          <main className="ed-stage" ref={stageRef}>
            {empty ? (
              <div className="ed-empty">
                <p>녹화하거나 파일을 열어 시작하세요.</p>
                <div className="row" style={{ justifyContent: "center" }}>
                  <button className="primary" disabled={busy} onClick={backToRecorder} title="녹화 틀을 띄워 녹화할 곳을 정합니다">
                    <IconCircleDot size={18} />녹화
                  </button>
                  <button disabled={busy} onClick={() => openPaths()}>
                    <IconFolderOpen size={18} />파일 열기
                  </button>
                  <button disabled={busy} onClick={() => openNewDialog("newBlank")}>빈 영상 만들기</button>
                  <button disabled={busy} onClick={() => openNewDialog("collage")}>콜라주</button>
                  <button onClick={() => setDialog("manage")}>
                    <IconFolders size={18} />파일 관리
                  </button>
                </div>
                <p className="hint">GIF·WebP·APNG·동영상·이미지 파일을 여기로 끌어 놓아도 됩니다.</p>
                <RecoveryList busy={busy} onOpen={(id) => exec(() => api.recoveryOpen(id))} onError={showToast} />
              </div>
            ) : mode === "crop" && cur ? (
              <CropView
                frame={cur}
                scale={scale}
                onCancel={() => setModeRaw(null)}
                onApply={(r) => {
                  setModeRaw(null);
                  run({ op: "crop", ...r });
                }}
                onError={showToast}
              />
            ) : mode === "filter" && cur ? (
              <FilterView
                frame={cur}
                scale={scale}
                onCancel={() => setModeRaw(null)}
                onApply={(filters, target) => {
                  setModeRaw(null);
                  run({ op: "filters", sel: target, filters });
                }}
                onError={showToast}
              />
            ) : mode === "draw" && cur ? (
              <DrawView
                frame={cur}
                scale={scale}
                onCancel={() => setModeRaw(null)}
                onApply={(rgba, w, h, target) => {
                  setModeRaw(null);
                  exec(() =>
                    api.applyOverlay(rgba, { sel: target, w, h, x: 0, y: 0, animIn: "none", animOut: "none", inMs: 0, outMs: 0 }),
                  );
                }}
              />
            ) : mode === "zoom" && cur ? (
              <ZoomView
                frame={cur}
                index={current}
                remainMs={total - curStart}
                scale={scale}
                onCancel={() => setModeRaw(null)}
                onApply={(zoom) => {
                  setModeRaw(null);
                  run({ op: "zoom", start: current, zoom });
                }}
                onError={showToast}
              />
            ) : mode === "insert" && cur ? (
              <InsertView
                frame={cur}
                scale={scale}
                onCancel={() => setModeRaw(null)}
                onApply={(chunks, target) => {
                  setModeRaw(null);
                  exec(() => api.applyOverlays(chunks.map((c) => ({ rgba: c.rgba, meta: { ...c.meta, sel: target } }))));
                }}
                onError={showToast}
              />
            ) : (
              cur && (
                <div className="ed-canvas" style={{ width: cur.width * scale, height: cur.height * scale }}>
                  <img
                    src={frameUrl(cur.id)}
                    width={cur.width * scale}
                    height={cur.height * scale}
                    style={{ imageRendering: scale >= 2 ? "pixelated" : "auto" }}
                    draggable={false}
                  />
                </div>
              )
            )}
            {busy && <div className="ed-busy">{busyText}</div>}
            {toast && <div className="ed-toast">{toast}</div>}
          </main>

          {!empty && cur && (
            <div className="ed-transport">
              <button
                className="ghost icon"
                onClick={() => {
                  setPlaying(false);
                  setCurrent((c) => Math.max(0, c - 1));
                }}
                title="이전 프레임 (←)"
              >
                <IconPlayerSkipBack size={18} />
              </button>
              <button className="ghost icon" onClick={() => setPlaying((p) => !p)} title="재생/정지 (Space)">
                {playing ? <IconPlayerPause size={18} /> : <IconPlayerPlay size={18} />}
              </button>
              <button
                className="ghost icon"
                onClick={() => {
                  setPlaying(false);
                  setCurrent((c) => Math.min(frames.length - 1, c + 1));
                }}
                title="다음 프레임 (→)"
              >
                <IconPlayerSkipForward size={18} />
              </button>
              <span className="ed-time">
                {formatMs(curStart)} <span>/ {formatMs(total)}</span>
              </span>
              <input
                type="range"
                className="ed-scrub"
                min={0}
                max={Math.max(0, frames.length - 1)}
                value={current}
                onChange={(e) => {
                  setPlaying(false);
                  setCurrent(Number(e.target.value));
                }}
                aria-label="재생 위치"
              />
              <span className="ed-pos">
                {current + 1} / {frames.length}
              </span>
              <button className="ghost icon" onClick={() => setZoom((z) => Math.max(ZOOM_MIN, (z === "fit" ? scale : z) / 1.25))} title="축소 (Ctrl+휠)">
                <IconMinus size={18} />
              </button>
              <button onClick={() => setZoom((z) => (z === "fit" ? 1 : "fit"))} title="화면에 맞춤 ↔ 100% (Ctrl+휠로 확대·축소)" className="zoom">
                {zoom === "fit" ? `맞춤 ${Math.round(scale * 100)}%` : `${Math.round(scale * 100)}%`}
              </button>
              <button className="ghost icon" onClick={() => setZoom((z) => Math.min(ZOOM_MAX, (z === "fit" ? scale : z) * 1.25))} title="확대 (Ctrl+휠)">
                <IconPlus size={18} />
              </button>
            </div>
          )}

          {!empty && (
            <FrameStrip
              frames={frames}
              current={current}
              sel={sel}
              onSelect={selectFrame}
              threeDigit={settings?.frameTime3digit ?? false}
              size="medium"
              onMove={moveFrames}
              onMenu={(i, x, y) => {
                if (!sel.includes(i)) selectFrame(i, { ctrlKey: false, shiftKey: false });
                setMenu({ x, y });
              }}
            />
          )}

          {!empty && (
            <div className="ed-ops">
              <button className="ghost" disabled={busy} onClick={() => run({ op: "delete", sel: target })} title="삭제 (Delete)">
                <IconTrash size={18} /><span className="lbl">삭제</span>
              </button>
              <button className="ghost" disabled={busy || sel.length === 0} onClick={() => run({ op: "deleteUnselected", sel })} title="선택하지 않은 프레임 삭제">
                <IconTrashX size={18} /><span className="lbl">선택 외 삭제</span>
              </button>
              <button className="ghost" disabled={busy} onClick={() => setDialog("cut")} title="구간만 남기기 (X)">
                <IconScissors size={18} /><span className="lbl">구간 잘라내기</span>
              </button>
              <button className="ghost" disabled={busy} onClick={() => setDialog("delay")} title="프레임 시간 설정">
                <IconClock size={18} /><span className="lbl">시간</span>
              </button>
              <span className="ed-speed" title="모든(선택) 프레임 속도">
                <IconGauge size={18} /><span className="lbl">속도</span>
                <button className="ghost icon" disabled={busy} onClick={() => run({ op: "scaleSpeed", sel, factor: 1.1 })} title="10% 느리게">
                  <IconMinus size={16} />
                </button>
                <button className="ghost icon" disabled={busy} onClick={() => run({ op: "scaleSpeed", sel, factor: 1 / 1.1 })} title="10% 빠르게">
                  <IconPlus size={16} />
                </button>
              </span>
              <button className="ghost" disabled={busy} onClick={() => run({ op: "reverse", sel })} title="선택 구간(없으면 전체)을 거꾸로">
                <IconArrowsLeftRight size={18} /><span className="lbl">역재생</span>
              </button>
              <button className="ghost" disabled={busy} onClick={() => run({ op: "yoyo", sel })} title="역재생 프레임을 뒤에 붙이기">
                <IconRepeat size={18} /><span className="lbl">왕복</span>
              </button>
              <button className="ghost" disabled={busy || sel.length < 2} onClick={() => run({ op: "merge", sel })} title="선택 프레임을 하나로">
                <IconStack2 size={18} /><span className="lbl">합치기</span>
              </button>
              <span className="grow" />
              <span className={`ed-count ${sel.length ? "on" : ""}`}>{sel.length ? `${sel.length}장 선택` : "선택 없음"}</span>
            </div>
          )}
        </section>

        {showSide && <aside className="ed-side" ref={setSideEl} />}
      </div>

      {menu && (
        <FrameMenu
          x={menu.x}
          y={menu.y}
          busy={busy}
          onClose={() => setMenu(null)}
          onPick={(v) => {
            setMenu(null);
            frameAction(v);
          }}
        />
      )}

      {dialog === "reduce" && settings && (
        <ModalInPanel.Provider value={true}>
          <ReduceDialog
            settings={settings}
            selCount={sel.length}
            total={frames.length}
            onClose={() => setDialog(null)}
            onApply={(op) => {
              setDialog(null);
              // 2장 이상 골랐을 때만 그 안에서 (패널 안내와 같게)
              run({ ...op, sel: sel.length > 1 ? sel : [] } as EditOp);
            }}
          />
        </ModalInPanel.Provider>
      )}
      {dialog === "resize" && cur && (
        <ModalInPanel.Provider value={true}>
          <ResizeDialog
            width={cur.width}
            height={cur.height}
            onClose={() => setDialog(null)}
            onApply={async (ops) => {
              setDialog(null);
              for (const op of ops) await run(op);
            }}
          />
        </ModalInPanel.Provider>
      )}
      {dialog === "delay" && cur && (
        <DelayDialog
          initial={cur.delayMs}
          selCount={sel.length}
          onClose={() => setDialog(null)}
          onApply={(ms) => {
            setDialog(null);
            run({ op: "setDelay", sel, ms });
          }}
        />
      )}
      {dialog === "cut" && (
        <CutRangeDialog
          count={frames.length}
          begin={sel.length ? sel[0] : 0}
          end={sel.length ? sel[sel.length - 1] : frames.length - 1}
          onClose={() => setDialog(null)}
          onApply={(begin, end) => {
            setDialog(null);
            run({ op: "cutRange", begin, end });
          }}
        />
      )}
      {dialog === "save" && settings && (
        <SaveDialog
          settings={settings}
          frames={frames}
          onClose={() => setDialog(null)}
          onSaved={() => {
            dirtyBeforeSave.current = view.dirty || savedFileDeleted;
            setSavedFileDeleted(false);
            setView((v) => ({ ...v, dirty: false }));
          }}
          onDeleted={() => setSavedFileDeleted(dirtyBeforeSave.current)}
          onNewRecording={async () => {
            if (blockWhileAppending()) return;
            setView(EMPTY);
            setDialog(null);
            await api.showRecorder();
          }}
        />
      )}
      {dialog === "upscale" && cur && (
        <ModalInPanel.Provider value={true}>
          <UpscaleDialog
            width={cur.width}
            height={cur.height}
            onClose={() => setDialog(null)}
            onApply={(factor) => {
              setDialog(null);
              run({ op: "upscale", factor });
            }}
          />
        </ModalInPanel.Provider>
      )}
      {dialog === "transition" && (
        <ModalInPanel.Provider value={true}>
          <TransitionDialog
            selCount={sel.length}
            onClose={() => setDialog(null)}
            onApply={(kind, ms) => {
              setDialog(null);
              run({ op: "transition", sel: sel.length > 1 ? sel : [], kind, ms });
            }}
          />
        </ModalInPanel.Provider>
      )}
      {importReq?.kind === "video" && (
        <VideoImportDialog
          path={importReq.path}
          info={importReq.info}
          defaultFps={settings?.fps ?? 15}
          onClose={() => {
            setImportReq(null);
            returnToRecorderIfCancelled();
          }}
          onImport={(start, end, fps, w, h) => {
            if (blockWhileAppending()) return;
            const path = importReq.path;
            setImportReq(null);
            exec(() => api.importVideo(path, start, end, fps, w, h));
          }}
        />
      )}
      {importReq?.kind === "images" && (
        <ImagesImportDialog
          paths={importReq.paths}
          onClose={() => {
            setImportReq(null);
            returnToRecorderIfCancelled();
          }}
          onImport={(ms, fit) => {
            if (blockWhileAppending()) return;
            const paths = importReq.paths;
            setImportReq(null);
            exec(() => api.importFiles(paths, ms, fit));
          }}
        />
      )}
      {dialog === "newBlank" && (
        <NewBlankDialog
          defaultFps={settings?.fps ?? 15}
          onClose={closeDialog}
          onCreate={async (w, h, fps, sec, color, transparent) => {
            if (blockWhileAppending()) return;
            if (askDiscard && !(await confirmAsk("저장하지 않은 내용이 있습니다. 버리고 새로 만들까요?", "버리고 만들기")))
              return;
            if (blockWhileAppending()) return;
            setDialog(null);
            exec(() => api.newBlank(w, h, fps, sec, hexToRgb(color), transparent));
          }}
        />
      )}
      {dialog === "collage" && (
        <CollageDialog
          current={cur ? { width: cur.width, height: cur.height, count: frames.length } : null}
          onClose={closeDialog}
          onError={showToast}
          onMake={async (paths, useCurrent, bg, o) => {
            if (blockWhileAppending()) return;
            if (
              askDiscard &&
              !useCurrent &&
              !(await confirmAsk("저장하지 않은 내용이 있습니다. 버리고 콜라주를 만들까요?", "버리고 만들기"))
            )
              return;
            if (blockWhileAppending()) return;
            setDialog(null);
            exec(() => api.makeCollage(paths, useCurrent, bg, o));
          }}
        />
      )}
      {dialog === "manage" && settings && (
        <ManageDialog
          settings={settings}
          onClose={closeDialog}
          onEdit={(path) => {
            if (blockWhileAppending()) return;
            setDialog(null);
            openPaths([path]);
          }}
        />
      )}
      {dialog === "exportFrames" && settings && (
        <ExportFramesDialog
          count={target.length}
          settings={settings}
          onClose={() => setDialog(null)}
          onExport={async (dir, prefix, format, openAfter) => {
            setDialog(null);
            try {
              const n = await api.exportFrames(target, dir, prefix, format);
              showToast(`${n}장을 내보냈습니다`);
              if (openAfter) await api.revealFile(dir);
            } catch (e) {
              showToast(errText(e));
            }
          }}
        />
      )}
      {dialog === "summary" && (
        <ModalInPanel.Provider value={true}>
          <SummaryDialog
            count={frames.length}
            totalMs={total}
            onClose={() => setDialog(null)}
            onApply={(options) => {
              setDialog(null);
              run({ op: "summarize", options });
            }}
          />
        </ModalInPanel.Provider>
      )}
      {dialog === "settings" && settings && (
        <SettingsDialog
          settings={settings}
          onClose={() => {
            setDialog(null);
            returnToRecorderIfCancelled();
          }}
          onSaved={(s) => setSettings(s)}
        />
      )}
    </div>
    </DirtyReport.Provider>
    </FrameContext.Provider>
    </SidePanelTarget.Provider>
  );
}

/** 썸네일 오른쪽 클릭 메뉴. 목록이 화면 아래에 있으니 커서 위쪽으로 펼친다. */
function FrameMenu({ x, y, busy, onPick, onClose }: { x: number; y: number; busy: boolean; onPick: (v: string) => void; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // 먼저 받아 멈춘다: 열린 도구 화면까지 Esc로 닫히지 않게
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  const items: [string, string, Icon, string][] = [...FRAME_ACTIONS, ["delete", "삭제", IconTrash, "Delete"]];
  return (
    <div className="ctx-back" onMouseDown={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }}>
      <div
        className="ctx-menu"
        role="menu"
        style={{ left: Math.max(4, Math.min(x, window.innerWidth - 270)), bottom: Math.max(4, window.innerHeight - y) }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {items.map(([v, label, Icon, key]) => (
          <button key={v} role="menuitem" disabled={busy} onClick={() => onPick(v)}>
            <Icon size={16} />
            {label}
            {key && <kbd>{key}</kbd>}
          </button>
        ))}
      </div>
    </div>
  );
}
