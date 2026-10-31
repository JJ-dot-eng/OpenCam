/** React 상태가 반영되기 전에도 붙이기를 하나만 받고, 지난 작업의 결과를 가려낸다. */
export type AppendGuard = { nextId: number; activeId: number | null; projectVersion: number };
export type AppendJob = { id: number; projectVersion: number };

export function createAppendGuard(): AppendGuard {
  return { nextId: 0, activeId: null, projectVersion: 0 };
}

export function blocksProjectChange(guard: AppendGuard): boolean {
  return guard.activeId !== null;
}

export function beginAppend(guard: AppendGuard): { guard: AppendGuard; job: AppendJob } | null {
  if (blocksProjectChange(guard)) return null;
  const id = guard.nextId + 1;
  return { guard: { ...guard, nextId: id, activeId: id }, job: { id, projectVersion: guard.projectVersion } };
}

export function isCurrentAppend(guard: AppendGuard, job: AppendJob): boolean {
  return guard.activeId === job.id && guard.projectVersion === job.projectVersion;
}

export function finishAppend(guard: AppendGuard, job: AppendJob): AppendGuard {
  return guard.activeId === job.id ? { ...guard, activeId: null } : guard;
}

/** 다른 창에서 프로젝트를 바꿔도 붙이기가 끝날 때까지 중복 요청은 계속 막는다. */
export function invalidateAppend(guard: AppendGuard): AppendGuard {
  return { ...guard, projectVersion: guard.projectVersion + 1 };
}
