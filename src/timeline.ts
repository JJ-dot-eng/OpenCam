// 도구 화면의 적용 범위 띠: 썸네일 줄 위에 "어디부터 어디까지 적용되는지"를 그리고, 끌어서 바꾼다.
// 여기는 계산만 한다 (노드 테스트에서 바로 부른다). 그리는 것은 FrameStrip.tsx.

/** 프레임 번호 구간 (처음·끝 모두 포함) */
export interface Span {
  start: number;
  end: number;
}

/** 띠 안에 진하게 칠하는 작은 구간 (예: 페이드가 걸리는 부분, 줌이 커지는 부분) */
export interface BandPart extends Span {
  label: string;
}

export interface Band extends Span {
  label: string;
  parts?: BandPart[];
  /** 떨어진 프레임을 고른 경우(필터) 실제로 적용되는 프레임들 */
  marks?: number[];
  /** 띠가 덮어야 하는 최소 프레임 수 (화면 전환은 2장) */
  minLen?: number;
  /**
   * 끌어서 범위를 바꿨다. edge는 잡은 곳. done은 손을 뗐을 때만 true:
   * 끄는 동안에는 띠·번호만 바꾸고, 미리보기 그림을 바꾸는 무거운 일은 손을 뗐을 때 한 번만 한다.
   */
  onChange: (start: number, end: number, edge: BandEdge, done: boolean) => void;
}

export type BandEdge = "start" | "end" | "move";

/** 프레임 번호들의 처음·끝. 한 번 훑어서 찾는다 (Math.min(...목록)은 수만 장이면 멈출 수 있어서). 비었으면 null */
export function bounds(indexes: number[]): Span | null {
  if (indexes.length === 0) return null;
  let start = indexes[0];
  let end = indexes[0];
  for (const i of indexes) {
    if (i < start) start = i;
    else if (i > end) end = i;
  }
  return { start, end };
}

/** start부터 시간을 쌓아 ms에 닿는 마지막 프레임 번호 (끝을 넘으면 마지막 프레임) */
export function framesForMs(delays: number[], start: number, ms: number): number {
  let sum = 0;
  for (let i = start; i < delays.length; i++) {
    sum += delays[i];
    if (sum >= ms) return i;
  }
  return Math.max(start, delays.length - 1);
}

/** end부터 거꾸로 시간을 쌓아 ms에 닿는 첫 프레임 번호 (floor보다 앞으로는 가지 않는다) */
export function framesBackMs(delays: number[], end: number, ms: number, floor = 0): number {
  let sum = 0;
  for (let i = end; i >= floor; i--) {
    sum += delays[i];
    if (sum >= ms) return i;
  }
  return Math.min(end, floor);
}

/** 구간의 재생 시간(ms) */
export function spanMs(delays: number[], start: number, end: number): number {
  let sum = 0;
  for (let i = Math.max(0, start); i <= end && i < delays.length; i++) sum += delays[i];
  return sum;
}

/** 띠를 끈 결과. value는 가장자리면 마우스 밑 프레임 번호, 옮기기면 처음 자리에서 움직인 장 수 */
export function dragBand(edge: BandEdge, orig: Span, value: number, total: number, minLen = 1): Span {
  const min = Math.max(1, Math.min(minLen, total));
  const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
  if (edge === "start") return { start: clamp(value, 0, orig.end - min + 1), end: orig.end };
  if (edge === "end") return { start: orig.start, end: clamp(value, orig.start + min - 1, total - 1) };
  const len = orig.end - orig.start;
  const start = clamp(orig.start + value, 0, Math.max(0, total - 1 - len));
  return { start, end: start + len };
}

/** a~b 프레임 번호 목록 (선택으로 쓴다) */
export function range(a: number, b: number): number[] {
  return Array.from({ length: Math.max(0, b - a + 1) }, (_, k) => a + k);
}

/** 오름차순으로 오는 번호 i를 이어진 구간 목록에 붙인다 (바로 뒤면 늘리고, 아니면 새 구간) */
function extend(out: Span[], i: number) {
  const r = out[out.length - 1];
  if (r && i <= r.end + 1) r.end = Math.max(r.end, i);
  else out.push({ start: i, end: i });
}

/** 고른 프레임 번호들을 이어진 구간으로 묶는다 (축소 보기에서 선택을 칠할 때). 이미 차례대로면 정렬하지 않는다 */
export function runs(indexes: number[]): Span[] {
  let sorted = true;
  for (let k = 1; k < indexes.length && sorted; k++) sorted = indexes[k - 1] <= indexes[k];
  const out: Span[] = [];
  for (const i of sorted ? indexes : [...indexes].sort((a, b) => a - b)) extend(out, i);
  return out;
}

/** 프레임마다 시작 시각(ms). n+1개라서 a~b 구간 길이는 at[b+1] - at[a] */
export function prefixMs(delays: number[]): number[] {
  const at = [0];
  for (const d of delays) at.push(at[at.length - 1] + d);
  return at;
}

// ---------- 효과 기록 ----------

/** 프레임에 입힌 효과 종류 (Rust `frame::tag`와 같은 비트, 테스트가 맞춰 본다). label은 왼쪽 도구 목록 이름과 같다 */
export const EFFECT_TAGS = [
  { bit: 1, key: "filter", label: "필터" },
  { bit: 2, key: "draw", label: "그리기" },
  { bit: 4, key: "insert", label: "텍스트 · 이미지" },
  { bit: 8, key: "transition", label: "화면 전환" },
  { bit: 16, key: "zoom", label: "줌" },
] as const;

export type EffectKey = (typeof EFFECT_TAGS)[number]["key"];

export interface EffectRow {
  key: EffectKey;
  label: string;
  /** 이 효과가 입혀진 이어진 구간들 */
  spans: Span[];
}

/** 프레임마다 붙은 효과 이름표로 효과 종류마다 한 줄을 만든다 (한 번 훑기). 한 번도 안 쓴 종류는 줄을 만들지 않는다. */
export function effectRows(tags: number[]): EffectRow[] {
  const spans: Span[][] = EFFECT_TAGS.map(() => []);
  tags.forEach((t, i) => {
    if (!t) return;
    EFFECT_TAGS.forEach((e, k) => {
      if (t & e.bit) extend(spans[k], i);
    });
  });
  return EFFECT_TAGS.flatMap((e, k) => (spans[k].length ? [{ key: e.key, label: e.label, spans: spans[k] }] : []));
}

/** 띠 이름 뒤에 붙는 말: "5~20번 · 1.60초" (ms는 구간 길이) */
export function spanLabel(s: Span, ms: number): string {
  const no = s.start === s.end ? `${s.start + 1}번` : `${s.start + 1}~${s.end + 1}번`;
  return `${no} · ${(ms / 1000).toFixed(2)}초`;
}

export function spanText(delays: number[], s: Span): string {
  return spanLabel(s, spanMs(delays, s.start, s.end));
}
