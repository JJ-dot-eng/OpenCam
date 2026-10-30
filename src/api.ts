// Rust 쪽 명령(src-tauri/src/lib.rs)을 부르는 함수와 자료형.

import { invoke } from "@tauri-apps/api/core";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface CaptureWindow {
  id: string;
  pid: number;
  title: string;
}

export interface Settings {
  alwaysOnTop: boolean;
  outputDir: string;
  fileNameRule: string;
  doNotAskDiscardRecording: boolean;
  doNotAskGif50fps: boolean;
  deleteToRecycleBin: boolean;
  checkUpdates: boolean;
  autoRecovery: boolean;

  fps: number;
  captureCursor: boolean;
  cursorHighlight: boolean;
  cursorHighlightColor: number;
  cursorClickFx: boolean;
  leftClickColor: number;
  rightClickColor: number;
  skipSameFrame: boolean;
  maxRecSec: number;
  recNoTimeLimit: boolean;
  recGlobalSnapshotHotkey: boolean;
  useMagnifier: boolean;
  useSemiTransparency: boolean;
  recToolbarBottom: boolean;
  recArrowKeyAccel: boolean;
  ignoreArrowKeysWhileRecording: boolean;
  gotoEditAfterRecording: boolean;
  sizePresets: [number, number][];
  savedPositions: ([number, number, number, number] | null)[];
  hotkeyRecord: string;
  hotkeyPause: string;
  hotkeyCtrlPrtsc: boolean;
  maxEditMemoryMb: number;
  recDirectSave: boolean;
  recDirectSaveQuality: number;
  recDirectSaveOpenFolder: boolean;
  recDirectSaveDropFrames: boolean;
  recDirectSaveNoTimeLimit: boolean;

  enableUndo: boolean;
  frameTime3digit: boolean;
  jpgExportQuality: number;

  gifQuality: number;
  gifDither: boolean;
  gifRemoveSimilarPixels: boolean;
  gifFiniteLoop: boolean;
  gifLoopCount: number;

  webpQuality: number;
  webpMethod: number;
  webpLossless: boolean;
  webpFiniteLoop: boolean;
  webpLoopCount: number;
  webpIphoneCompat: boolean;

  apngFiniteLoop: boolean;
  apngLoopCount: number;
  apng256Colors: boolean;

  avifQuality: number;

  webmQuality: number;
  mp4Quality: number;
  mp4ReplaceTransparent: boolean;
  transparentReplaceColor: number;

  limitFps: boolean;
  limitFpsValue: number;
  skipSimilar: boolean;
  skipSimilarPercent: number;
  skipSimilarTimeLimit: boolean;
  skipSimilarTimeLimitMs: number;
  resizePercent: number;

  embedProgress: boolean;
  embedProgressColor: number;
  embedProgressBgColor: number;
  embedProgressLineWidth: number;
  embedProgressMinSec: number;

  reduceSimilarity: number;
  reduceTolerance: number;
  reduceTimeLimit: boolean;
  reduceTimeLimitMs: number;
  reduceKeepSpeed: boolean;
}

export interface RecStatus {
  recording: boolean;
  paused: boolean;
  frames: number;
  elapsedMs: number;
  memoryMb: number;
  stopReason: string | null;
}

export interface FrameInfo {
  id: number;
  width: number;
  height: number;
  delayMs: number;
}

/** 비정상 종료로 남은 작업 */
export interface RecoveryEntry {
  id: string;
  /** 마지막 자동 저장 시각 (Unix ms) */
  savedAt: number;
  frames: number;
  width: number;
  height: number;
  totalMs: number;
}

export interface ProjectView {
  frames: FrameInfo[];
  canUndo: boolean;
  canRedo: boolean;
  dirty: boolean;
  selection: number[] | null;
  message: string | null;
  /** 편집에 쓰는 메모리(바이트, 되돌리기·다시 실행 기록 포함) */
  memoryBytes: number;
}

export type Format = "gif" | "webp" | "apng" | "mp4" | "webm" | "png" | "avif";

export const FORMAT_EXT: Record<Format, string> = {
  gif: "gif",
  webp: "webp",
  apng: "png",
  mp4: "mp4",
  webm: "webm",
  png: "png",
  avif: "avif",
};

export interface ExportOptions {
  format: Format;
  quality: number;
  limitFps: number | null;
  skipSimilar: number | null;
  skipSimilarTimeLimitMs: number | null;
  resizePercent: number;
  embedProgress: boolean;
  /** GIF에서 꼭 지킬 색 (특정 색 보호) */
  protectColors: Rgb[];
  /** GIF를 20ms 간격(50 FPS)으로 다시 뽑아 저장 (이번 저장에만) */
  gifFit50fps?: boolean;
}

export type Rotate = "left" | "right" | "half" | "flipH" | "flipV";

export type Rgb = [number, number, number];

/** 흐리게·모자이크를 일부에만 쓸 때의 영역 (프레임 픽셀) */
export interface Region extends Rect {
  ellipse: boolean;
}

export type Filter =
  | { kind: "brightness"; value: number }
  | { kind: "contrast"; value: number }
  | { kind: "gamma"; value: number }
  | { kind: "saturation"; value: number }
  | { kind: "grayscale" }
  | { kind: "sepia"; amount: number }
  | { kind: "invert" }
  | { kind: "sharpen"; amount: number }
  | { kind: "blur"; radius: number; region: Region | null }
  | { kind: "mosaic"; size: number; region: Region | null }
  | { kind: "vignette"; amount: number }
  | { kind: "scanline"; gap: number; strength: number }
  | { kind: "temperature"; value: number }
  | { kind: "monotone"; color: Rgb }
  | { kind: "duotone"; dark: Rgb; light: Rgb }
  | { kind: "tiltShift"; center: number; band: number; radius: number }
  | { kind: "irisBlur"; cx: number; cy: number; rx: number; ry: number; radius: number; feather: number }
  | { kind: "curve"; shadows: number; midtones: number; highlights: number }
  | { kind: "channels"; brightness: number; red: number; green: number; blue: number }
  | { kind: "whiteBalance"; reference: Rgb | null }
  | { kind: "denoise"; strength: number }
  | { kind: "bloom"; radius: number; amount: number; shiny: boolean }
  | { kind: "cartoon"; edges: number; levels: number }
  | { kind: "print"; threshold: number }
  | { kind: "ascii"; cell: number; color: boolean }
  | { kind: "afterimage"; strength: number }
  | { kind: "chromaKey"; color: Rgb; range: number; adjacent: boolean; replace: Rgb | null }
  | { kind: "eraser"; region: Region | null };

export type TransitionKind = "fadeInWhite" | "fadeInBlack" | "fadeOutWhite" | "fadeOutBlack" | "flash" | "crossfadeLoop";

export type ZoomScale = "goodFit" | "half" | "threeQuarter" | "full";

/** 줌 애니메이션 (기본값: 중앙으로, 그림자 60%, 총 2초, 등장·퇴장 0.3초) */
export interface ZoomFx {
  region: Region;
  scale: ZoomScale;
  toCenter: boolean;
  shadow: boolean;
  spread: number;
  totalMs: number;
  introMs: number;
  outroMs: number;
}

/** 영상 요약 (기본값: 50%, 프레임당 최대 300ms, 프레임 시간 20%로) */
export interface SummaryOptions {
  rate: number;
  limitMs: number | null;
  /** null이면 총 재생 시간 유지 */
  timePercent: number | null;
}

export interface VideoInfo {
  width: number;
  height: number;
  durationMs: number;
  fps: number;
}

/** 여러 장을 한 크기로 맞추는 방법: 비율 유지(남는 곳 투명) / 꽉 채우기 */
export type Fit = "contain" | "stretch";

export const VIDEO_EXTS = ["mp4", "webm", "mkv", "mov", "avi", "wmv", "m4v", "flv", "mpg", "mpeg", "ts"];

export function extOf(path: string): string {
  const m = /\.([^.\\/]+)$/.exec(path);
  return m ? m[1].toLowerCase() : "";
}

export type PlayMode = "hold" | "repeat" | "sync" | "sequence";

export interface CollageOptions {
  cols: number;
  rows: number;
  width: number;
  height: number;
  margin: number;
  gap: number;
  roundness: number;
  background: Rgb;
  backgroundAlpha: number;
  drawBorder: boolean;
  borderColor: Rgb;
  play: PlayMode;
}

export interface FileEntry {
  name: string;
  path: string;
  size: number;
  modifiedMs: number;
}

export type FileFilter = [string, string[]];

export type Anim =
  | "none"
  | "fade"
  | "zoomIn"
  | "zoomOut"
  | "left"
  | "right"
  | "top"
  | "bottom"
  | "fadeSmall"
  | "rotateBig"
  | "rotateSmall";

export interface OverlayMeta {
  sel: number[];
  w: number;
  h: number;
  x: number;
  y: number;
  animIn: Anim;
  animOut: Anim;
  inMs: number;
  outMs: number;
}

export type EditOp =
  | { op: "delete"; sel: number[] }
  | { op: "deleteUnselected"; sel: number[] }
  | { op: "cutRange"; begin: number; end: number }
  | { op: "reverse"; sel: number[] }
  | { op: "yoyo"; sel: number[] }
  | { op: "merge"; sel: number[] }
  | { op: "setDelay"; sel: number[]; ms: number }
  | { op: "scaleSpeed"; sel: number[]; factor: number }
  | { op: "move"; sel: number[]; dir: number }
  | { op: "moveToEnd"; sel: number[]; toFront: boolean; copy: boolean }
  | { op: "moveTo"; sel: number[]; to: number }
  | { op: "copy"; sel: number[] }
  | { op: "cut"; sel: number[] }
  | { op: "paste"; at: number }
  | { op: "reduce"; sel: number[]; mode: "even" | "odd" | "nth"; n: number; keepSpeed: boolean }
  | {
      op: "reduceSimilar";
      sel: number[];
      similarity: number;
      tolerance: number;
      timeLimitMs: number | null;
      keepSpeed: boolean;
    }
  | { op: "crop"; x: number; y: number; w: number; h: number }
  | { op: "resize"; w: number; h: number; smooth: boolean }
  | { op: "rotate"; r: Rotate }
  | { op: "undo" }
  | { op: "redo" }
  | { op: "restoreInitial" }
  | { op: "setInitial" }
  | { op: "filters"; sel: number[]; filters: Filter[] }
  | { op: "transition"; sel: number[]; kind: TransitionKind; ms: number }
  | { op: "zoom"; start: number; zoom: ZoomFx }
  | { op: "summarize"; options: SummaryOptions }
  | { op: "upscale"; factor: number };

export const api = {
  /** 녹화 틀 창에 구멍을 뚫는다. keep: 창에서 남길 부분(툴바·틀) */
  setHole: (r: Rect, keep: Rect[]) => invoke<void>("set_hole", { ...r, keep }),
  monitors: () => invoke<Rect[]>("monitors"),
  resizeMouseDown: () => invoke<boolean>("resize_mouse_down"),
  /** 크기 조절 중 돋보기. 마우스 버튼을 떼면 스스로 닫힌다. */
  startMagnifier: (scale: number) => invoke<void>("start_magnifier", { scale }),
  /** 시작할 때 생긴 문제(단축키 등록 실패 등)를 한 번 꺼낸다 */
  takeStartupNotice: () => invoke<string | null>("take_startup_notice"),
  getFullCapture: () => invoke<Rect | null>("get_full_capture"),
  setFullCapture: (monitor: Rect | null) => invoke<void>("set_full_capture", { monitor }),
  captureWindows: () => invoke<CaptureWindow[]>("capture_windows"),
  captureWindowBounds: (id: string, pid: number) => invoke<{ rect: Rect; activated: boolean }>("capture_window_bounds", { id, pid }),
  beginAreaSelection: () => invoke<void>("begin_area_selection"),
  finishAreaSelection: (rect: Rect | null) => invoke<void>("finish_area_selection", { rect }),
  recStart: () => invoke<void>("rec_start"),
  recStop: () => invoke<void>("rec_stop"),
  recPause: () => invoke<boolean>("rec_pause"),
  recStatus: () => invoke<RecStatus | null>("rec_status"),
  recSaveImage: () => invoke<string>("rec_save_image"),
  recSnapshotGlobalActive: () => invoke<boolean>("rec_snapshot_global_active"),
  showRecorder: () => invoke<void>("show_recorder"),
  quitApp: () => invoke<void>("quit_app"),
  recoveryList: () => invoke<RecoveryEntry[]>("recovery_list"),
  recoveryThumb: (id: string) => invoke<ArrayBuffer>("recovery_thumb", { id }),
  recoveryOpen: (id: string) => invoke<ProjectView>("recovery_open", { id }),
  recoveryDiscard: (id: string) => invoke<void>("recovery_discard", { id }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  projectView: () => invoke<ProjectView>("project_view"),
  edit: (op: EditOp) => invoke<ProjectView>("edit", { op }),
  trimBounds: () => invoke<[number, number, number, number] | null>("trim_bounds"),
  defaultSavePath: (ext: string) => invoke<string>("default_save_path", { ext }),
  pickSavePath: (current: string, ext: string) => invoke<string | null>("pick_save_path", { current, ext }),
  pickFolder: (current: string) => invoke<string | null>("pick_folder", { current }),
  exportProject: (opts: ExportOptions, path: string) => invoke<number>("export_project", { opts, path }),
  estimateSize: (opts: ExportOptions) => invoke<number>("estimate_size", { opts }),
  hasFfmpeg: () => invoke<boolean>("has_ffmpeg"),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  deleteFile: (path: string) => invoke<void>("delete_file", { path }),
  /** 필터를 적용한 미리보기 PNG */
  filterPreview: (id: number, filters: Filter[]) => invoke<ArrayBuffer>("filter_preview", { id, filters }),
  /** 줌이 가장 크게 된 순간의 미리보기 PNG */
  zoomPreview: (id: number, zoom: ZoomFx) => invoke<ArrayBuffer>("zoom_preview", { id, zoom }),
  /** 영상 요약을 하면 남는 프레임 수와 총 시간 */
  summaryPreview: (options: SummaryOptions) =>
    invoke<{ frames: number; totalMs: number }>("summary_preview", { options }),
  pickOpenFiles: () => invoke<string[] | null>("pick_open_files"),
  probeVideo: (path: string) => invoke<VideoInfo>("probe_video", { path }),
  /** 동영상의 한 장면 PNG (구간 고르기용) */
  videoFrame: (path: string, atMs: number) => invoke<ArrayBuffer>("video_frame", { path, atMs: Math.round(atMs) }),
  importFiles: (paths: string[], stillMs: number, fit: Fit) => invoke<ProjectView>("import_files", { paths, stillMs, fit }),
  importVideo: (path: string, startMs: number, endMs: number, fps: number, width: number, height: number) =>
    invoke<ProjectView>("import_video", {
      path,
      startMs: Math.round(startMs),
      endMs: Math.round(endMs),
      fps,
      width: Math.round(width),
      height: Math.round(height),
    }),
  newBlank: (width: number, height: number, fps: number, sec: number, color: Rgb, transparent: boolean) =>
    invoke<ProjectView>("new_blank", { width, height, fps, sec, color, transparent }),
  makeCollage: (paths: string[], useCurrent: boolean, backgroundImage: string | null, options: CollageOptions) =>
    invoke<ProjectView>("make_collage", { paths, useCurrent, backgroundImage, options }),
  listFiles: (dir: string) => invoke<FileEntry[]>("list_files", { dir }),
  renameFile: (path: string, newName: string) => invoke<string>("rename_file", { path, newName }),
  fileBytes: (path: string) => invoke<ArrayBuffer>("file_bytes", { path }),
  copyFrameImage: (index: number) => invoke<void>("copy_frame_image", { index }),
  pasteClipboardImage: (index: number, replace: boolean) => invoke<ProjectView>("paste_clipboard_image", { index, replace }),
  clipboardImagePng: () => invoke<ArrayBuffer>("clipboard_image_png"),
  saveFrameImage: (index: number, path: string) => invoke<void>("save_frame_image", { index, path }),
  exportFrames: (indices: number[], dir: string, prefix: string, format: string) =>
    invoke<number>("export_frames", { indices, dir, prefix, format }),
  pickSaveFile: (current: string, filters: FileFilter[]) => invoke<string | null>("pick_save_file", { current, filters }),
  pickFile: (filters: FileFilter[]) => invoke<string | null>("pick_file", { filters }),
  exportSettings: (path: string) => invoke<void>("export_settings", { path }),
  importSettings: (path: string) => invoke<Settings>("import_settings", { path }),
  /** 그림(RGBA 픽셀)을 프레임들에 덧씌운다. 픽셀은 JSON이 아니라 그대로 보낸다. */
  applyOverlay: (rgba: Uint8Array, meta: OverlayMeta) =>
    invoke<ProjectView>("apply_overlay", rgba, { headers: { "x-overlay": JSON.stringify(meta) } }),
  /** 여러 개(레이어)를 한 번에: 픽셀은 순서대로 이어 붙여 보내고, 앞의 것이 아래에 깔린다 */
  applyOverlays: (items: { rgba: Uint8Array; meta: OverlayMeta }[]) => {
    const total = items.reduce((n, it) => n + it.rgba.length, 0);
    const body = new Uint8Array(total);
    let at = 0;
    for (const it of items) {
      body.set(it.rgba, at);
      at += it.rgba.length;
    }
    return invoke<ProjectView>("apply_overlay", body, {
      headers: { "x-overlay": JSON.stringify(items.map((it) => it.meta)) },
    });
  },
};

const isWindows = navigator.userAgent.includes("Windows");

/** 프레임 이미지 주소. 픽셀이 바뀌면 id가 바뀌므로 브라우저 캐시를 그대로 써도 된다. */
export function frameUrl(id: number, thumb = false): string {
  const base = isWindows ? "http://frame.localhost/" : "frame://localhost/";
  return `${base}${id}${thumb ? "/t" : ""}`;
}

/** 설정의 색(BGR 정수) ↔ #rrggbb */
export function bgrToHex(c: number): string {
  const r = c & 0xff;
  const g = (c >> 8) & 0xff;
  const b = (c >> 16) & 0xff;
  return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}

export function hexToBgr(hex: string): number {
  const v = parseInt(hex.slice(1), 16);
  const r = (v >> 16) & 0xff;
  const g = (v >> 8) & 0xff;
  const b = v & 0xff;
  return r | (g << 8) | (b << 16);
}

export function formatMs(ms: number): string {
  const s = ms / 1000;
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, "0")}:${(s - m * 60).toFixed(1).padStart(4, "0")}`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

export function hexToRgb(hex: string): Rgb {
  const v = parseInt(hex.slice(1), 16);
  return [(v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff];
}

export function rgbToHex([r, g, b]: Rgb): string {
  return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}

/** 파일 크기·날짜 표시 (오늘은 시:분:초, 올해는 월-일 시:분, 그 밖은 년-월-일) */
export function formatFileDate(ms: number): string {
  const d = new Date(ms);
  const now = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  if (d.toDateString() === now.toDateString()) return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  if (d.getFullYear() === now.getFullYear()) return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export const IMAGE_FILTERS: FileFilter[] = [["이미지", ["png", "jpg", "jpeg", "bmp", "webp", "gif"]]];
