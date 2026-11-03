// 오른쪽 도구 패널이 함께 쓰는 부품: 섹션, 이름표+입력 줄, 단위가 붙은 숫자칸, 색 칸, 고르기 버튼 묶음,
// 적용 범위, 바닥(결과 요약·취소·실행). 모든 도구 패널이 같은 높이·간격·단위 표기를 쓰게 한다.

import { createContext, ReactNode, useContext, useEffect, useRef, useState } from "react";
import { IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { errText } from "./api";
import { Band, BandEdge, spanText } from "./timeline";

/** 지금 편집 상황: 보고 있는 프레임(0부터), 전체 장수, 고른 프레임 */
export interface FrameCtx {
  current: number;
  total: number;
  sel: number[];
}
export const FrameContext = createContext<FrameCtx>({ current: 0, total: 0, sel: [] });

/** 도구 패널에 아직 적용하지 않은 변경이 있는지 편집 창에 알린다 (다른 도구로 바꿀 때 물어보려고) */
export const DirtyReport = createContext<(dirty: boolean) => void>(() => {});

export function useReportDirty(dirty: boolean) {
  const report = useContext(DirtyReport);
  useEffect(() => report(dirty), [dirty, report]);
  useEffect(() => () => report(false), [report]);
}

/** 썸네일 줄의 적용 범위 띠와 이어 주는 것 (편집 창이 준다) */
export interface TimelineCtx {
  /** 프레임마다 시간(ms) */
  delays: number[];
  setBand: (b: Band | null) => void;
  /** start~end를 고른 프레임으로 */
  selectRange: (start: number, end: number) => void;
  setCurrent: (i: number) => void;
}
export const TimelineContext = createContext<TimelineCtx>({ delays: [], setBand: () => {}, selectRange: () => {}, setCurrent: () => {} });

/** 열린 도구의 적용 범위를 썸네일 줄 위에 띠로 보여 준다. 도구가 닫히면 띠도 사라진다. */
export function useBand(band: Omit<Band, "onChange"> | null, onChange: Band["onChange"]) {
  const { setBand } = useContext(TimelineContext);
  const cb = useRef(onChange);
  cb.current = onChange;
  const key = band ? JSON.stringify(band) : "";
  useEffect(() => {
    setBand(band ? { ...band, onChange: (a, b, edge) => cb.current(a, b, edge) } : null);
    // 띠 내용(key)이 바뀔 때만 다시 알린다
  }, [key, setBand]);
  useEffect(() => () => setBand(null), [setBand]);
}

/** 설정이 바뀌면 잠깐(120ms) 기다렸다가 미리보기 PNG를 받아 Blob URL로 넘긴다. load가 null이면 받지 않는다.
 *  지난 URL은 새 그림으로 바뀐 뒤에 해제되도록 2초 있다가 푼다. deps가 바뀔 때만 다시 받는다. */
export function usePngPreview(
  load: (() => Promise<ArrayBuffer>) | null,
  onUrl: (url: string) => void,
  onError: (msg: string) => void,
  deps: unknown[],
) {
  useEffect(() => {
    if (!load) return;
    let url: string | null = null;
    let alive = true;
    const t = window.setTimeout(async () => {
      try {
        const buf = await load();
        if (!alive) return;
        url = URL.createObjectURL(new Blob([buf], { type: "image/png" }));
        onUrl(url);
      } catch (e) {
        onError(errText(e));
      }
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
      if (url) window.setTimeout(() => URL.revokeObjectURL(url!), 2000);
    };
  }, deps);
}

// ---------- 섹션 ----------

/** 관련 설정 묶음. collapsible이면 접을 수 있고, 접혀 있을 때 summary(지금 설정 요약)를 보여 준다. */
export function Section({
  title,
  summary,
  collapsible,
  defaultOpen = true,
  action,
  children,
}: {
  title: string;
  summary?: ReactNode;
  collapsible?: boolean;
  defaultOpen?: boolean;
  /** 제목 줄 오른쪽 버튼 (예: + 추가) */
  action?: ReactNode;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const shown = !collapsible || open;
  return (
    <section className="psec">
      <div className="psec-head">
        {collapsible ? (
          <button className="psec-toggle" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            {open ? <IconChevronDown size={16} /> : <IconChevronRight size={16} />}
            <span>{title}</span>
          </button>
        ) : (
          <h3>{title}</h3>
        )}
        {collapsible && !open && summary && <span className="psec-sum">{summary}</span>}
        {action && <span className="psec-action">{action}</span>}
      </div>
      {shown && <div className="psec-body">{children}</div>}
    </section>
  );
}

// ---------- 줄 ----------

/** 왼쪽 이름표 + 오른쪽 입력. sub면 바로 위 설정에 딸린 하위 설정(들여쓰기). */
export function Field({ label, sub, children }: { label: ReactNode; sub?: boolean; children: ReactNode }) {
  return (
    <div className={`pfield ${sub ? "sub" : ""}`}>
      <span className="pfield-label">{label}</span>
      <div className="pfield-ctl">{children}</div>
    </div>
  );
}

/** 단위가 칸 안에 붙은 숫자 입력. 비우면 NaN을 넘긴다(받는 쪽에서 거른다). */
export function NumInput({
  value,
  onChange,
  min,
  max,
  step,
  unit,
  disabled,
  label,
  title,
  wide,
  narrow,
}: {
  value: number;
  onChange: (v: number) => void;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  disabled?: boolean;
  /** 화면 읽기용 이름 (이름표가 따로 없을 때) */
  label?: string;
  title?: string;
  wide?: boolean;
  narrow?: boolean;
}) {
  return (
    <span className={`pnum ${disabled ? "dis" : ""} ${wide ? "wide" : ""} ${narrow ? "narrow" : ""}`} title={title}>
      <input
        type="number"
        aria-label={label}
        value={Number.isFinite(value) ? value : ""}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value === "" ? NaN : Number(e.target.value))}
      />
      {unit && <em>{unit}</em>}
    </span>
  );
}

/** 막대 + 숫자칸 (어느 쪽으로 바꿔도 된다) */
export function Slider({
  value,
  onChange,
  min,
  max,
  step = 1,
  unit,
  disabled,
  label,
}: {
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  unit?: string;
  disabled?: boolean;
  label: string;
}) {
  return (
    <span className="pslider">
      <input
        type="range"
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <NumInput
        value={value}
        min={min}
        max={max}
        step={step}
        unit={unit}
        disabled={disabled}
        label={label}
        onChange={(v) => Number.isFinite(v) && onChange(Math.max(min, Math.min(max, v)))}
      />
    </span>
  );
}

/** 색 고르기 + #RRGGBB 입력 */
export function ColorInput({
  value,
  onChange,
  disabled,
  label,
}: {
  value: string;
  onChange: (v: string) => void;
  disabled?: boolean;
  label: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <span className={`pcolor ${disabled ? "dis" : ""}`}>
      <input type="color" aria-label={label} value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)} />
      <input
        aria-label={`${label} 값`}
        value={draft ?? value.toUpperCase()}
        disabled={disabled}
        spellCheck={false}
        onChange={(e) => {
          const v = e.target.value.trim();
          setDraft(v);
          const hex = v.startsWith("#") ? v : `#${v}`;
          if (/^#[0-9a-fA-F]{6}$/.test(hex)) onChange(hex.toLowerCase());
        }}
        onBlur={() => setDraft(null)}
      />
    </span>
  );
}

/** 여러 개 중 하나 고르기 (설정만 바꾼다. 실행은 바닥의 실행 버튼) */
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  label,
  iconOnly,
}: {
  value: T;
  onChange: (v: T) => void;
  /** [값, 이름, 아이콘?, 못 고르는 이유?] */
  options: [T, string, ReactNode?, string?][];
  label: string;
  iconOnly?: boolean;
}) {
  return (
    <div className="pseg" role="radiogroup" aria-label={label}>
      {options.map(([v, name, icon, why]) => (
        <button
          key={v}
          role="radio"
          aria-checked={value === v}
          className={value === v ? "on" : ""}
          disabled={!!why}
          title={why ?? (iconOnly ? name : undefined)}
          onClick={() => onChange(v)}
        >
          {icon}
          {(!iconOnly || !icon) && <span>{name}</span>}
        </button>
      ))}
    </div>
  );
}

// ---------- 적용 범위 ----------

export type Scope = "current" | "selection" | "all";

/** 범위에 맞는 프레임 번호 목록 (편집 명령의 sel: 빈 목록이면 전체) */
export function scopeSel(scope: Scope, f: FrameCtx): number[] {
  return scope === "current" ? [f.current] : scope === "selection" ? f.sel : [];
}

/** 고른 프레임이 있으면 그 범위, 없으면 전체를 기본으로 */
export function defaultScope(f: FrameCtx): Scope {
  return f.sel.length > 0 ? "selection" : "all";
}

/**
 * 범위를 사람이 읽는 말로. span이면 고른 프레임의 처음~끝 구간 전체(덧씌우기·화면 전환),
 * 아니면 고른 프레임만(필터).
 */
export function scopeText(scope: Scope, f: FrameCtx, span: boolean): string {
  if (scope === "current") return `지금 프레임(${f.current + 1}번)에만 적용`;
  if (scope === "selection" && f.sel.length > 0) {
    const a = Math.min(...f.sel) + 1;
    const b = Math.max(...f.sel) + 1;
    return span ? `선택 구간 ${a}~${b}번(${b - a + 1}장)에 적용` : `선택한 ${f.sel.length}장에 적용`;
  }
  return `전체 ${f.total}장에 적용`;
}

/** 범위가 덮는 프레임 구간 (떨어진 프레임을 골랐으면 처음~끝) */
export function scopeSpan(scope: Scope, f: FrameCtx): { start: number; end: number } {
  if (scope === "current") return { start: f.current, end: f.current };
  if (scope === "selection" && f.sel.length > 0) return { start: Math.min(...f.sel), end: Math.max(...f.sel) };
  return { start: 0, end: Math.max(0, f.total - 1) };
}

/** 띠를 끌었을 때: 그 구간을 고르고, 잡은 쪽 끝 프레임을 보여 준다 */
export function bandSelect(t: TimelineCtx, start: number, end: number, edge: BandEdge) {
  t.selectRange(start, end);
  t.setCurrent(edge === "end" ? end : start);
}

export function ScopePicker({
  value,
  onChange,
  span,
  name,
}: {
  value: Scope;
  onChange: (s: Scope) => void;
  span: boolean;
  /** 썸네일 줄 띠에 쓰는 도구 이름 */
  name: string;
}) {
  const f = useContext(FrameContext);
  const t = useContext(TimelineContext);
  const s = scopeSpan(value, f);
  // 필터처럼 고른 프레임만 바꾸는 도구는 떨어진 프레임을 따로 표시한다
  const gaps = !span && value === "selection" && f.sel.length > 0 && f.sel.length !== s.end - s.start + 1;
  const label = gaps ? `${name} · 선택한 ${f.sel.length}장` : `${name} · ${spanText(t.delays, s)}`;
  useBand({ ...s, label, marks: gaps ? f.sel : undefined }, (a, b, edge) => {
    bandSelect(t, a, b, edge);
    onChange("selection");
  });
  const noSel = f.sel.length === 0 ? "프레임 목록에서 프레임을 먼저 고르세요 (Ctrl·Shift로 여러 장)" : undefined;
  return (
    <div className="pscope">
      <span className="pfoot-label">적용 범위</span>
      <Segmented
        label="적용 범위"
        value={value}
        onChange={onChange}
        options={[
          ["current", "현재 프레임"],
          ["selection", span ? "선택 구간" : "선택 프레임", undefined, noSel],
          ["all", "전체 프레임"],
        ]}
      />
      <p className="pfoot-info">{scopeText(value, f, span)}</p>
    </div>
  );
}

// ---------- 바닥 ----------

/**
 * 패널 바닥: (범위 고르기 또는 적용 대상 안내) / 결과 요약 / 취소·실행.
 * 실행할 수 없으면 그 이유를 버튼 위에 보여 준다.
 */
export function PanelFoot({
  scope,
  target,
  summary,
  applyLabel = "적용",
  disabledReason,
  onCancel,
  onApply,
}: {
  scope?: ReactNode;
  /** 범위를 고를 수 없는 도구: 실제로 어디에 적용되는지 */
  target?: ReactNode;
  summary?: ReactNode;
  applyLabel?: string;
  /** 있으면 실행 버튼을 막고 이유를 보여 준다 */
  disabledReason?: string | null;
  onCancel: () => void;
  onApply: () => void;
}) {
  return (
    <>
      {scope}
      {target && (
        <div className="pscope">
          <span className="pfoot-label">적용 대상</span>
          <p className="pfoot-info">{target}</p>
        </div>
      )}
      {summary && <p className="pfoot-sum">{summary}</p>}
      {disabledReason && <p className="pfoot-why">{disabledReason}</p>}
      <div className="pfoot-btns">
        <button onClick={onCancel}>취소 (Esc)</button>
        <button className="primary" disabled={!!disabledReason} title={disabledReason ?? undefined} onClick={onApply}>
          {applyLabel}
        </button>
      </div>
    </>
  );
}
