// 편집 창의 작은 대화상자들.

import { createContext, ReactNode, useContext, useEffect, useRef, useState } from "react";
import {
  api,
  EditOp,
  errText,
  Rotate,
  Settings,
  SummaryOptions,
  TransitionKind,
} from "./api";

import SidePanel from "./SidePanel";
import { Field, FrameContext, NumInput, PanelFoot, Section, Segmented, Slider, useReportDirty } from "./Panel";

/** true면 Modal을 창 가운데가 아니라 편집 창 오른쪽 패널에 그린다 (왼쪽 도구 목록에서 연 도구) */
export const ModalInPanel = createContext(false);

/** 열린 대화상자 순서. 겹쳐 있으면 맨 위 것만 Enter·Esc를 받는다. */
const modalStack: number[] = [];
let nextModalId = 1;

/** 대화상자가 하나라도 열려 있으면 true (편집 창 단축키를 막을 때) */
export function isModalOpen(): boolean {
  return modalStack.length > 0;
}

export function Modal({
  title,
  children,
  onClose,
  onOk,
  okText = "적용",
  okDisabled,
  wide,
  hideCancel,
  desc,
  target,
  summary,
  disabledReason,
  dirty,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  onOk?: () => void;
  okText?: string;
  okDisabled?: boolean;
  wide?: boolean;
  hideCancel?: boolean;
  /** 오른쪽 패널에 띄울 때만 쓰는 것들: 짧은 설명, 적용 대상, 결과 요약, 실행할 수 없는 이유, 적용 안 한 변경 여부 */
  desc?: ReactNode;
  target?: ReactNode;
  summary?: ReactNode;
  disabledReason?: string | null;
  dirty?: boolean;
}) {
  const inPanel = useContext(ModalInPanel);
  const id = useRef(0);
  if (id.current === 0) id.current = nextModalId++;
  const blocked = okDisabled || !!disabledReason;
  useReportDirty(inPanel && !!dirty);

  useEffect(() => {
    const me = id.current;
    modalStack.push(me);
    return () => {
      const i = modalStack.indexOf(me);
      if (i >= 0) modalStack.splice(i, 1);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (modalStack[modalStack.length - 1] !== id.current) return;
      if (e.key === "Escape") onClose();
      else if (e.key === "Enter" && onOk && !blocked && (e.target as HTMLElement).tagName !== "TEXTAREA") {
        e.preventDefault();
        onOk();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, onOk, blocked]);

  if (inPanel)
    return (
      <SidePanel
        title={title}
        desc={desc}
        foot={
          onOk && (
            <PanelFoot
              target={target}
              summary={summary}
              applyLabel={okText}
              disabledReason={disabledReason ?? (okDisabled ? "설정 값을 확인하세요" : null)}
              onCancel={onClose}
              onApply={onOk}
            />
          )
        }
      >
        {children}
      </SidePanel>
    );

  return (
    <div className="modal-back" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal ${wide ? "wide" : ""}`} role="dialog" aria-label={title}>
        <h2>{title}</h2>
        <div className="modal-body">{children}</div>
        {onOk && (
          <div className="modal-foot">
            {!hideCancel && <button onClick={onClose}>취소</button>}
            <button className="primary" onClick={onOk} disabled={blocked}>
              {okText}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function NumField({
  label,
  value,
  onChange,
  min,
  max,
  suffix,
  disabled,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  min?: number;
  max?: number;
  suffix?: string;
  disabled?: boolean;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <input
        type="number"
        value={Number.isFinite(value) ? value : ""}
        min={min}
        max={max}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value === "" ? NaN : Number(e.target.value))}
      />
      {suffix && <em>{suffix}</em>}
    </label>
  );
}

export function Check({
  label,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className="check">
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, Number.isFinite(v) ? v : lo));

// ---------- 프레임 (Q): 홀수·짝수·N번째·비슷한 프레임 지우기 ----------

type ReduceOp = Extract<EditOp, { op: "reduce" } | { op: "reduceSimilar" }>;
/** 유니온의 각 타입에서 sel을 뺀다 */
type WithoutSel<T> = T extends unknown ? Omit<T, "sel"> : never;

type ReduceMode = "none" | "even" | "odd" | "nth" | "similar";

export function ReduceDialog({
  settings,
  selCount,
  total,
  onClose,
  onApply,
}: {
  settings: Settings;
  selCount: number;
  /** 전체 프레임 수 (지운 뒤 남는 장수를 미리 보여 주려고) */
  total: number;
  onClose: () => void;
  onApply: (op: WithoutSel<ReduceOp>) => void;
}) {
  // "none"(원본)은 아무것도 지우지 않는 처음 상태
  const [mode, setMode] = useState<ReduceMode>("none");
  const [n, setN] = useState(3);
  const [similarity, setSimilarity] = useState(settings.reduceSimilarity);
  const [tolerance, setTolerance] = useState(settings.reduceTolerance);
  const [timeLimit, setTimeLimit] = useState(settings.reduceTimeLimit);
  const [timeLimitMs, setTimeLimitMs] = useState(settings.reduceTimeLimitMs);
  const [keepSpeed, setKeepSpeed] = useState(settings.reduceKeepSpeed);

  // 대상 프레임 수와, 홀수·짝수·N번째로 지웠을 때 지워지는 수 (비슷한 프레임은 해 봐야 안다)
  const count = selCount > 1 ? selCount : total;
  const nn = clamp(n, 2, 1000);
  const dropped = mode === "even" ? Math.floor(count / 2) : mode === "odd" ? Math.ceil(count / 2) : mode === "nth" ? Math.floor(count / nn) : 0;

  const ok = () =>
    mode !== "none" &&
    onApply(
      mode === "similar"
        ? {
            op: "reduceSimilar",
            similarity: clamp(similarity, 0, 50),
            tolerance: clamp(tolerance, 0, 30),
            timeLimitMs: timeLimit ? clamp(timeLimitMs, 10, 600000) : null,
            keepSpeed,
          }
        : { op: "reduce", mode, n: nn, keepSpeed },
    );

  const modes: [ReduceMode, string][] = [
    ["none", "원본 (지우지 않음)"],
    ["odd", "홀수 프레임 삭제 (1·3·5…번째)"],
    ["even", "짝수 프레임 삭제 (2·4·6…번째)"],
    ["nth", "N번째 프레임마다 삭제"],
    ["similar", "비슷한 프레임 삭제"],
  ];

  return (
    <Modal
      title="프레임 삭제"
      desc="정한 규칙에 맞는 프레임을 지워 장수를 줄입니다."
      onClose={onClose}
      onOk={ok}
      okText="삭제하기"
      disabledReason={mode === "none" ? "삭제 방식을 고르세요" : null}
      dirty={mode !== "none"}
      target={selCount > 1 ? `선택한 ${selCount}장 안에서 지웁니다` : `전체 ${total}장에서 지웁니다`}
      summary={
        mode === "similar"
          ? "비슷한 프레임 수는 실행해 봐야 알 수 있습니다"
          : dropped > 0
            ? `${total}장 → ${total - dropped}장 (${dropped}장 삭제)`
            : null
      }
    >
      <Section title="삭제 방식">
        <div className="radio-list">
          {modes.map(([k, label]) => (
            <label key={k} className="check">
              <input type="radio" name="reduce" checked={mode === k} onChange={() => setMode(k)} />
              {label}
            </label>
          ))}
        </div>
        {mode === "nth" && (
          <>
            <Field label="N" sub>
              <NumInput value={n} min={2} label="몇 번째마다" onChange={setN} />
            </Field>
            <p className="hint">
              {nn}이면 {nn}·{nn * 2}·{nn * 3}…번째 프레임을 지웁니다.
            </p>
          </>
        )}
        {mode === "similar" && (
          <>
            <Field label="바뀐 픽셀" sub>
              <NumInput value={similarity} min={0} max={50} unit="%" label="바뀐 픽셀 비율 (이 값 이하면 비슷함)" onChange={setSimilarity} />
            </Field>
            <Field label="색 차이" sub>
              <NumInput value={tolerance} min={0} max={30} label="색 차이 허용치" onChange={setTolerance} />
            </Field>
            <Field label="시간 제한" sub>
              <input type="checkbox" aria-label="비슷한 프레임 시간 제한" checked={timeLimit} onChange={(e) => setTimeLimit(e.target.checked)} />
              <NumInput value={timeLimitMs} min={10} unit="ms" disabled={!timeLimit} label="시간 제한" onChange={setTimeLimitMs} />
            </Field>
            <p className="hint">
              바뀐 픽셀이 이 비율 이하이면 비슷한 프레임으로 봅니다 (0%면 완전히 같은 것만). 색 차이는 이만큼까지 다른 색을 같은 색으로 셉니다.
            </p>
          </>
        )}
      </Section>
      <Section title="재생 시간">
        <Check label="재생 속도 유지 (지운 시간을 앞 프레임에 더함)" checked={keepSpeed} onChange={setKeepSpeed} />
      </Section>
    </Modal>
  );
}

// ---------- 크기 / 회전 (R) ----------

/** 회전·뒤집기 고르기 ("none"은 그대로) */
type Turn = "none" | Rotate;

export function ResizeDialog({
  width,
  height,
  onClose,
  onApply,
}: {
  width: number;
  height: number;
  onClose: () => void;
  /** 크기 바꾸기 → 회전 순서로 실행할 편집들 */
  onApply: (ops: EditOp[]) => void;
}) {
  const [w, setW] = useState(width);
  const [h, setH] = useState(height);
  const [keep, setKeep] = useState(true);
  const [smooth, setSmooth] = useState(true);
  const [turn, setTurn] = useState<Turn>("none");
  const pct = Math.round((w / width) * 100);

  const setWidth = (v: number) => {
    setW(v);
    if (keep && Number.isFinite(v)) setH(Math.max(1, Math.round((v * height) / width)));
  };
  const setHeight = (v: number) => {
    setH(v);
    if (keep && Number.isFinite(v)) setW(Math.max(1, Math.round((v * width) / height)));
  };
  const setPct = (p: number) => {
    if (!Number.isFinite(p)) return;
    setW(Math.max(1, Math.round((width * p) / 100)));
    setH(Math.max(1, Math.round((height * p) / 100)));
  };
  const valid = w >= 1 && h >= 1 && w <= 16384 && h <= 16384;
  const resized = valid && (Math.round(w) !== width || Math.round(h) !== height);
  // 90도 돌리면 가로세로가 바뀐다
  const swap = turn === "left" || turn === "right";
  const [rw, rh] = resized ? [Math.round(w), Math.round(h)] : [width, height];
  const turnName: Record<Turn, string> = {
    none: "",
    left: "왼쪽 90° 회전",
    right: "오른쪽 90° 회전",
    half: "180° 회전",
    flipH: "좌우 뒤집기",
    flipV: "상하 뒤집기",
  };

  const ops: EditOp[] = [];
  if (resized) ops.push({ op: "resize", w: rw, h: rh, smooth });
  if (turn !== "none") ops.push({ op: "rotate", r: turn });

  return (
    <Modal
      title="크기 · 회전"
      desc="크기와 회전을 정한 뒤 실행하면 한꺼번에 바뀝니다. 미리보기에는 실행 전까지 원래 모습이 보입니다."
      onClose={onClose}
      onOk={() => ops.length && onApply(ops)}
      okText="바꾸기"
      dirty={ops.length > 0}
      disabledReason={!valid ? "가로·세로는 1~16384px 사이로 입력하세요" : ops.length === 0 ? "크기나 회전을 바꾸세요" : null}
      target="전체 프레임에 적용"
      summary={
        ops.length > 0 && (
          <>
            {width} × {height} → <b>{swap ? `${rh} × ${rw}` : `${rw} × ${rh}`}</b>
            {turn !== "none" && ` · ${turnName[turn]}`}
          </>
        )
      }
    >
      <Section title="크기">
        <Field label="가로">
          <NumInput value={w} min={1} unit="px" wide label="가로" onChange={setWidth} />
        </Field>
        <Field label="세로">
          <NumInput value={h} min={1} unit="px" wide label="세로" onChange={setHeight} />
        </Field>
        <Field label="비율">
          <NumInput value={pct} min={1} unit="%" wide label="비율" onChange={setPct} />
        </Field>
        <Field label="빠른 비율" sub>
          <div className="prow">
            {[50, 75, 150, 200].map((p) => (
              <button key={p} className={pct === p ? "on" : ""} onClick={() => setPct(p)} title={`가로·세로를 ${p}%로 정하기`}>
                {p}%
              </button>
            ))}
          </div>
        </Field>
        <Check label="가로세로 비율 유지" checked={keep} onChange={setKeep} />
      </Section>
      <Section title="보간 방식">
        <Segmented
          label="보간 방식"
          value={smooth ? "smooth" : "pixel"}
          onChange={(v) => setSmooth(v === "smooth")}
          options={[
            ["smooth", "부드럽게"],
            ["pixel", "픽셀 그대로"],
          ]}
        />
        <p className="hint">
          {smooth ? "사진·영상에 알맞습니다." : "픽셀 아트·도트 그림을 계단 모양 그대로 키울 때 씁니다."} 글자를 또렷하게 키우려면 왼쪽
          "고품질 확대"를 쓰세요.
        </p>
      </Section>
      <Section title="회전 · 뒤집기">
        <div className="radio-list">
          {(["none", "left", "right", "half", "flipH", "flipV"] as Turn[]).map((t) => (
            <label key={t} className="check">
              <input type="radio" name="turn" checked={turn === t} onChange={() => setTurn(t)} />
              {t === "none" ? "그대로" : turnName[t]}
            </label>
          ))}
        </div>
      </Section>
    </Modal>
  );
}

// ---------- 고품질 확대 ----------

/** 고품질 확대(초해상도): 배율을 고르고 결과 크기를 확인한 뒤 실행 */
export function UpscaleDialog({
  width,
  height,
  onClose,
  onApply,
}: {
  width: number;
  height: number;
  onClose: () => void;
  onApply: (factor: number) => void;
}) {
  const [factor, setFactor] = useState<"2" | "3" | "4">("2");
  const k = Number(factor);
  return (
    <Modal
      title="고품질 확대"
      desc="학습된 모델(ESPCN)로 글자·경계를 또렷하게 키웁니다. 큰 녹화는 시간이 걸립니다."
      onClose={onClose}
      onOk={() => onApply(k)}
      okText="확대하기"
      target="전체 프레임에 적용"
      summary={
        <>
          {width} × {height} → <b>{width * k} × {height * k}</b>
        </>
      }
    >
      <Section title="배율">
        <Segmented
          label="배율"
          value={factor}
          onChange={setFactor}
          options={[
            ["2", "2배"],
            ["3", "3배"],
            ["4", "4배"],
          ]}
        />
        <p className="hint">배율이 클수록 오래 걸리고 메모리를 많이 씁니다 (픽셀 수가 배율의 제곱만큼 늘어남).</p>
      </Section>
    </Modal>
  );
}

// ---------- 프레임 시간 ----------

export function DelayDialog({
  initial,
  selCount,
  onClose,
  onApply,
}: {
  initial: number;
  selCount: number;
  onClose: () => void;
  onApply: (ms: number) => void;
}) {
  const [ms, setMs] = useState(initial);
  const valid = ms >= 10 && ms <= 600000;
  return (
    <Modal title="프레임 시간" onClose={onClose} onOk={() => onApply(Math.round(ms))} okDisabled={!valid}>
      <p className="hint">{selCount > 0 ? `선택한 ${selCount}장에 적용합니다.` : "모든 프레임에 적용합니다."}</p>
      <div className="row">
        <NumField label="시간" value={ms} min={10} suffix="ms" onChange={setMs} />
        <NumField label="= FPS" value={Math.round((1000 / ms) * 10) / 10} min={0.1} onChange={(f) => f > 0 && setMs(Math.round(1000 / f))} />
      </div>
      <div className="row">
        {[20, 33, 50, 66, 100, 200, 500, 1000].map((v) => (
          <button key={v} onClick={() => setMs(v)}>
            {v}
          </button>
        ))}
      </div>
    </Modal>
  );
}

// ---------- 구간 잘라내기 (X) ----------

export function CutRangeDialog({
  count,
  begin,
  end,
  onClose,
  onApply,
}: {
  count: number;
  begin: number;
  end: number;
  onClose: () => void;
  onApply: (begin: number, end: number) => void;
}) {
  const [b, setB] = useState(begin + 1);
  const [e, setE] = useState(end + 1);
  const valid = b >= 1 && e >= b && e <= count;
  return (
    <Modal title="구간 잘라내기" onClose={onClose} onOk={() => onApply(b - 1, e - 1)} okDisabled={!valid} okText="이 구간만 남기기">
      <p className="hint">고른 구간만 남기고 나머지 프레임을 지웁니다.</p>
      <div className="row">
        <NumField label="시작" value={b} min={1} max={count} onChange={setB} />
        <NumField label="끝" value={e} min={1} max={count} onChange={setE} />
        <span className="hint">/ 전체 {count}장</span>
      </div>
    </Modal>
  );
}

// ---------- 화면 전환 (T) ----------

/** 화면 전환: 종류 → (페이드면) 색 → 시간 */
type TrKind = "fadeIn" | "fadeOut" | "flash" | "crossfadeLoop";

export function TransitionDialog({
  selCount,
  onClose,
  onApply,
}: {
  selCount: number;
  onClose: () => void;
  onApply: (kind: TransitionKind, ms: number) => void;
}) {
  const f = useContext(FrameContext);
  const [kind, setKind] = useState<TrKind>("fadeIn");
  const [color, setColor] = useState<"Black" | "White">("Black");
  const [ms, setMs] = useState(500);
  const fade = kind === "fadeIn" || kind === "fadeOut";
  const real: TransitionKind = fade ? (`${kind}${color}` as TransitionKind) : kind;
  const about: Record<TrKind, string> = {
    fadeIn: `처음 ${ms}ms 동안 ${color === "Black" ? "검은" : "흰"} 화면에서 서서히 나타납니다.`,
    fadeOut: `마지막 ${ms}ms 동안 ${color === "Black" ? "검은" : "흰"} 화면으로 서서히 사라집니다.`,
    flash: `처음 ${ms}ms 동안 흰빛이 번쩍였다가 사라집니다.`,
    crossfadeLoop: `마지막 ${ms}ms를 첫 프레임과 겹쳐, 반복 재생할 때 끝과 처음이 자연스럽게 이어집니다.`,
  };
  const span = selCount > 1;
  const a = span ? Math.min(...f.sel) + 1 : 1;
  const b = span ? Math.max(...f.sel) + 1 : f.total;
  return (
    <Modal
      title="화면 전환"
      onClose={onClose}
      onOk={() => onApply(real, Math.max(10, Math.round(ms)))}
      disabledReason={!(ms >= 10) ? "효과 시간은 10ms 이상으로 입력하세요" : null}
      target={span ? `선택 구간 ${a}~${b}번(${b - a + 1}장)에 적용` : `전체 ${f.total}장에 적용`}
      summary={about[kind]}
    >
      <Section title="효과">
        <div className="radio-list">
          {(
            [
              ["fadeIn", "페이드 인 (나타나기)"],
              ["fadeOut", "페이드 아웃 (사라지기)"],
              ["flash", "플래시 (번쩍)"],
              ["crossfadeLoop", "반복 겹치기 (끝을 처음과 겹침)"],
            ] as const
          ).map(([k, label]) => (
            <label key={k} className="check">
              <input type="radio" name="tr" checked={kind === k} onChange={() => setKind(k)} />
              {label}
            </label>
          ))}
        </div>
        {fade && (
          <Field label="색" sub>
            <Segmented
              label="페이드 색"
              value={color}
              onChange={setColor}
              options={[
                ["Black", "검은색"],
                ["White", "흰색"],
              ]}
            />
          </Field>
        )}
      </Section>
      <Section title="시간">
        <Field label="효과 시간">
          <NumInput value={ms} min={10} unit="ms" wide label="효과 시간" onChange={setMs} />
        </Field>
        {ms > 0 && (
          <p className="hint">효과 시간이 {span ? "선택 구간" : "영상"}보다 길면 효과가 끝나기 전에 {span ? "구간" : "영상"}이 끝납니다.</p>
        )}
      </Section>
    </Modal>
  );
}

// ---------- 영상 요약 (S) ----------

/** 영상 요약 처음 값 (이와 다르면 적용하지 않은 변경이 있는 것) */
const DEFAULT_SUMMARY: SummaryOptions = { rate: 50, limitMs: 300, timePercent: 20 };

export function SummaryDialog({
  count,
  totalMs,
  onClose,
  onApply,
}: {
  count: number;
  totalMs: number;
  onClose: () => void;
  onApply: (o: SummaryOptions) => void;
}) {
  const [rate, setRate] = useState(50);
  const [limit, setLimit] = useState(true);
  const [limitMs, setLimitMs] = useState(300);
  const [keepTotal, setKeepTotal] = useState(false);
  const [percent, setPercent] = useState(20);
  const [result, setResult] = useState<{ frames: number; totalMs: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const options: SummaryOptions = {
    rate: clamp(rate, 1, 100),
    limitMs: limit ? clamp(limitMs, 10, 600000) : null,
    timePercent: keepTotal ? null : clamp(percent, 1, 100),
  };
  const key = JSON.stringify(options);

  // 결과(남는 프레임 수, 총 시간)를 미리 계산해 보여준다
  useEffect(() => {
    let alive = true;
    const t = window.setTimeout(async () => {
      try {
        const r = await api.summaryPreview(options);
        if (alive) {
          setResult(r);
          setError(null);
        }
      } catch (e) {
        if (alive) setError(errText(e));
      }
    }, 150);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [key]);

  return (
    <Modal
      title="영상 요약"
      desc="화면이 많이 바뀐 프레임만 남겨 짧게 줄입니다."
      onClose={onClose}
      onOk={() => onApply(options)}
      okText="요약하기"
      dirty={key !== JSON.stringify(DEFAULT_SUMMARY)}
      target={`전체 ${count}장에 적용`}
      summary={
        <>
          {count}장 · {(totalMs / 1000).toFixed(1)}초
          {result && (
            <>
              {" "}
              → <b>{result.frames}장 · {(result.totalMs / 1000).toFixed(1)}초</b>
            </>
          )}
        </>
      }
    >
      <Section title="남길 프레임">
        <Field label="요약률">
          <Slider value={rate} min={1} max={100} unit="%" label="요약률" onChange={setRate} />
        </Field>
        <p className="hint">전체 프레임 중 이 비율만큼 남깁니다 (낮을수록 짧아짐).</p>
      </Section>
      <Section title="재생 시간">
        <div className="radio-list">
          <label className="check">
            <input type="radio" name="sumtime" checked={!keepTotal} onChange={() => setKeepTotal(false)} />
            남은 프레임의 시간을 비율로 바꾸기
          </label>
        </div>
        <Field label="시간 비율" sub>
          <NumInput value={percent} min={1} max={100} unit="%" label="프레임 시간 비율" disabled={keepTotal} onChange={setPercent} />
        </Field>
        <div className="radio-list">
          <label className="check">
            <input type="radio" name="sumtime" checked={keepTotal} onChange={() => setKeepTotal(true)} />
            총 재생 시간 유지 (지운 프레임의 시간을 앞 프레임이 이어받음)
          </label>
        </div>
        <Field label="최대 시간">
          <input type="checkbox" aria-label="프레임당 최대 시간 제한" checked={limit} onChange={(e) => setLimit(e.target.checked)} />
          <NumInput value={limitMs} min={10} unit="ms" wide label="프레임당 최대 시간" disabled={!limit} onChange={setLimitMs} />
        </Field>
        <p className="hint">최대 시간을 켜면 한 프레임이 이보다 오래 보이지 않게 자릅니다.</p>
      </Section>
      {error && <p className="error">{error}</p>}
    </Modal>
  );
}
