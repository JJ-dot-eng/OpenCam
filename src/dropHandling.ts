import { sortByFileName } from "./fileOrder.ts";

export const VIDEO_EXTS = ["mp4", "webm", "mkv", "mov", "avi", "wmv", "m4v", "flv", "mpg", "mpeg", "ts"];
/** 움직이는 그림일 수 있는 확장자 (하나만 열면 옵션 없이 바로 불러온다) */
const ANIMATED_EXTS = ["gif", "webp", "png", "apng"];

export function extOf(path: string): string {
  const m = /\.([^.\\/]+)$/.exec(path);
  return m ? m[1].toLowerCase() : "";
}

export type OpenPathsDecision =
  | { kind: "reject"; message: string }
  | { kind: "video"; path: string }
  | { kind: "import"; paths: string[] }
  | { kind: "images"; paths: string[] };

/** 빈 목록과 취소·버리기 확인은 부르는 쪽에서 먼저 처리한다. */
export function classifyOpenPaths(paths: string[]): OpenPathsDecision {
  const sorted = sortByFileName(paths);
  const videos = sorted.filter((p) => VIDEO_EXTS.includes(extOf(p)));
  if (videos.length > 0) {
    if (sorted.length > 1) return { kind: "reject", message: "동영상은 하나씩 따로 열어 주세요" };
    return { kind: "video", path: videos[0] };
  }
  if (sorted.length === 1 && ANIMATED_EXTS.includes(extOf(sorted[0]))) return { kind: "import", paths: sorted };
  return { kind: "images", paths: sorted };
}

/** 파일을 끌어 오는 동안: 놓으면 맨 뒤에 붙는지, 새로 여는지, 받지 않는지(null) */
export type DropKind = null | "append" | "open";

export function getDropKind(state: {
  appending: boolean;
  busy: boolean;
  dialog: unknown;
  importReq: unknown;
  menu: unknown;
  mode: unknown;
  isModalOpen: () => boolean;
  frameCount: number;
}): DropKind {
  return state.appending || state.busy || state.dialog || state.importReq || state.menu || state.mode || state.isModalOpen()
    ? null : state.frameCount > 0 ? "append" : "open";
}

export type FileDropEvent =
  | { type: "enter" | "drop"; paths: string[] }
  | { type: "over" | "leave" };

/** 화면과 무관한 콜백으로 힌트를 지우는 순서와 파일을 받는 순서를 지킨다. */
export function handleFileDrop(event: FileDropEvent, handlers: {
  kind: () => DropKind;
  setHint: (kind: DropKind) => void;
  run: (paths: string[]) => void;
  showToast: (message: string) => void;
}): void {
  if (event.type === "enter" || event.type === "over") return handlers.setHint(handlers.kind());
  handlers.setHint(null);
  if (event.type !== "drop" || event.paths.length === 0) return;
  if (handlers.kind()) handlers.run(event.paths);
  else handlers.showToast("처리 중이거나 창·도구가 열려 있어 파일을 받지 않았습니다");
}
