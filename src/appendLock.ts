import { beginAppend, blocksProjectChange, finishAppend, isCurrentAppend } from "./appendGuard.ts";
import type { AppendGuard } from "./appendGuard.ts";
import { sortByFileName } from "./fileOrder.ts";

export function appendBlockMessage(guard: AppendGuard): string | null {
  return blocksProjectChange(guard) ? "파일을 붙이는 중입니다. 끝난 뒤 다시 해 주세요" : null;
}

export function isProjectDialog(dialog: string | null): boolean {
  return dialog === "newBlank" || dialog === "collage";
}

/** 붙이는 동안 모든 단축키를 막되, 열기·콜라주에는 까닭을 알린다. */
export function appendShortcutAction(guard: AppendGuard, event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
}): "allow" | "block" | "notify" {
  if (!blocksProjectChange(guard)) return "allow";
  const ctrl = event.ctrlKey || event.metaKey;
  const k = event.key.toLowerCase();
  return (ctrl && k === "o") || (!ctrl && k === "g") ? "notify" : "block";
}

/** 화면 갱신과 파일 읽기는 콜백으로 받아, 실제 붙이기의 잠금·결과 처리 순서를 함께 검사한다. */
export async function runAppend<View>(paths: string[], handlers: {
  guard: { current: AppendGuard };
  blockWhileAppending: () => unknown;
  setBusy: (busy: boolean) => void;
  setPlaying: (playing: boolean) => void;
  setBusyText: (text: string) => void;
  appendFiles: (paths: string[]) => Promise<{ view: View; skipped: { name: string; reason: string }[] }>;
  projectView: () => Promise<View>;
  applyStaleView: (view: View) => void;
  applyView: (view: View) => void;
  alert: (message: string) => Promise<unknown>;
  showError: (error: unknown) => void;
}): Promise<void> {
  const started = beginAppend(handlers.guard.current);
  if (!started) {
    handlers.blockWhileAppending();
    return;
  }
  // 첫 await 전에 잠가서 같은 순간에 들어오는 두 번째 drop도 막는다.
  handlers.guard.current = started.guard;
  const job = started.job;
  handlers.setBusy(true);
  handlers.setPlaying(false);
  try {
    const sorted = sortByFileName(paths);
    const r = await handlers.appendFiles(sorted);
    if (!isCurrentAppend(handlers.guard.current, job)) {
      // 화면 작업 번호가 낡아도 Rust는 붙였을 수 있으므로 지금 내용을 다시 읽는다.
      const v = await handlers.projectView();
      handlers.applyStaleView(v);
      return;
    }
    handlers.applyView(r.view);
    const skipped = r.skipped;
    if (skipped.length === 0) return;
    const shown = skipped.slice(0, 15).map((s) => `· ${s.name}: ${s.reason}`);
    if (skipped.length > shown.length) shown.push(`… 그 밖에 ${skipped.length - shown.length}개`);
    const head =
      skipped.length === sorted.length
        ? "파일을 붙이지 못했습니다. 편집 중인 프레임은 그대로입니다."
        : `${skipped.length}개 파일은 붙이지 못하고 건너뛰었습니다.`;
    await handlers.alert(`${head}\n\n${shown.join("\n")}`);
  } catch (e) {
    if (isCurrentAppend(handlers.guard.current, job)) handlers.showError(e);
  } finally {
    if (handlers.guard.current.activeId === job.id) {
      handlers.guard.current = finishAppend(handlers.guard.current, job);
      handlers.setBusy(false);
      handlers.setBusyText("처리 중…");
    }
  }
}
