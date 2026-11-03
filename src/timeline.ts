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
  /** 끌어서 범위를 바꿨다. edge는 잡은 곳 */
  onChange: (start: number, end: number, edge: BandEdge) => void;
}

export type BandEdge = "start" | "end" | "move";

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

/** 고른 프레임 번호들을 이어진 구간으로 묶는다 (축소 보기에서 선택을 칠할 때) */
export function runs(indexes: number[]): Span[] {
  const out: Span[] = [];
  for (const i of [...indexes].sort((a, b) => a - b)) {
    const r = out[out.length - 1];
    if (r && i <= r.end + 1) r.end = Math.max(r.end, i);
    else out.push({ start: i, end: i });
  }
  return out;
}

/** 띠 이름 뒤에 붙는 말: "5~20번 · 1.60초" */
export function spanText(delays: number[], s: Span): string {
  const no = s.start === s.end ? `${s.start + 1}번` : `${s.start + 1}~${s.end + 1}번`;
  return `${no} · ${(spanMs(delays, s.start, s.end) / 1000).toFixed(2)}초`;
}
