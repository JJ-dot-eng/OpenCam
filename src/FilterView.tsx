// 필터 화면 (F): 여러 필터를 골라 지금 프레임으로 미리 보고, 선택 프레임이나 전체에 적용한다.

import { useContext, useEffect, useRef, useState } from "react";
import { IconEye } from "@tabler/icons-react";
import { api, errText, Filter, FrameInfo, frameUrl, hexToRgb, Region } from "./api";
import { Check, isModalOpen } from "./dialogs";
import { promptAsk } from "./ask";
import SidePanel from "./SidePanel";
import { ColorInput, defaultScope, Field, FrameContext, PanelFoot, Scope, ScopePicker, scopeSel, Section, Slider, useReportDirty } from "./Panel";

type Kind = Filter["kind"];

interface Param {
  key: string;
  label: string;
  min: number;
  max: number;
  step?: number;
  def: number;
  unit?: string;
}

interface Def {
  kind: Kind;
  label: string;
  params?: Param[];
  colors?: { key: string; label: string; def: string }[];
  checks?: { key: string; label: string; def: boolean }[];
  /** 일부 영역에만 쓸 수 있는 필터 */
  region?: boolean;
  /** 영역이 꼭 있어야 하는 필터 (지우개) */
  regionRequired?: boolean;
  /** 화면 값 → Rust 값으로 바꿀 때 더 손볼 것 */
  build?: (f: Record<string, unknown>) => void;
  hint?: string;
}

const DEFS: Def[] = [
  { kind: "brightness", label: "밝기", params: [{ key: "value", label: "", min: -100, max: 100, def: 20 }] },
  { kind: "contrast", label: "대비", params: [{ key: "value", label: "", min: -100, max: 100, def: 20 }] },
  { kind: "gamma", label: "감마", params: [{ key: "value", label: "", min: 0.1, max: 3, step: 0.05, def: 1.2 }] },
  { kind: "saturation", label: "채도", params: [{ key: "value", label: "", min: -100, max: 100, def: 30 }] },
  { kind: "temperature", label: "색온도", params: [{ key: "value", label: "", min: -100, max: 100, def: 30 }] },
  { kind: "grayscale", label: "흑백" },
  { kind: "sepia", label: "세피아", params: [{ key: "amount", label: "", min: 0, max: 100, def: 100 }] },
  { kind: "invert", label: "색 반전" },
  { kind: "monotone", label: "모노톤", colors: [{ key: "color", label: "색", def: "#4f8cff" }] },
  {
    kind: "duotone",
    label: "듀오톤",
    colors: [
      { key: "dark", label: "어두운 색", def: "#1b1464" },
      { key: "light", label: "밝은 색", def: "#ffcf5c" },
    ],
  },
  { kind: "sharpen", label: "선명하게", params: [{ key: "amount", label: "", min: 0, max: 100, def: 40 }] },
  { kind: "blur", label: "흐리게", region: true, params: [{ key: "radius", label: "반경", min: 1, max: 50, def: 6 }] },
  { kind: "mosaic", label: "모자이크", region: true, params: [{ key: "size", label: "칸 크기", min: 2, max: 80, def: 12 }] },
  { kind: "vignette", label: "비네팅", params: [{ key: "amount", label: "", min: 0, max: 100, def: 60 }] },
  {
    kind: "tiltShift",
    label: "틸트시프트",
    params: [
      { key: "center", label: "위치", unit: "%", min: 0, max: 100, def: 50 },
      { key: "band", label: "또렷한 폭", unit: "%", min: 5, max: 100, def: 30 },
      { key: "radius", label: "흐림", min: 1, max: 40, def: 8 },
    ],
  },
  {
    kind: "scanline",
    label: "스캔라인",
    params: [
      { key: "gap", label: "간격", min: 2, max: 10, def: 3 },
      { key: "strength", label: "세기", min: 0, max: 100, def: 40 },
    ],
  },
  {
    kind: "irisBlur",
    label: "배경 흐림",
    params: [
      { key: "cx", label: "가로 위치", unit: "%", min: 0, max: 100, def: 50 },
      { key: "cy", label: "세로 위치", unit: "%", min: 0, max: 100, def: 50 },
      { key: "rx", label: "가로 크기", unit: "%", min: 5, max: 100, def: 30 },
      { key: "ry", label: "세로 크기", unit: "%", min: 5, max: 100, def: 30 },
      { key: "radius", label: "흐림", min: 1, max: 40, def: 8 },
      { key: "feather", label: "번짐", unit: "%", min: 1, max: 200, def: 40 },
    ],
  },
  {
    kind: "curve",
    label: "색상 곡선",
    params: [
      { key: "shadows", label: "어두운 곳", min: 0, max: 255, def: 50 },
      { key: "midtones", label: "중간", min: 0, max: 255, def: 128 },
      { key: "highlights", label: "밝은 곳", min: 0, max: 255, def: 205 },
    ],
  },
  {
    kind: "channels",
    label: "색상 채널",
    params: [
      { key: "brightness", label: "밝기", min: -100, max: 100, def: 0 },
      { key: "red", label: "빨강", min: -100, max: 100, def: 15 },
      { key: "green", label: "녹색", min: -100, max: 100, def: 0 },
      { key: "blue", label: "파랑", min: -100, max: 100, def: 0 },
    ],
  },
  {
    kind: "whiteBalance",
    label: "화이트밸런스",
    checks: [{ key: "auto", label: "자동으로 맞추기", def: true }],
    colors: [{ key: "reference", label: "기준 색 (이 색이 흰색이 됨)", def: "#f0f0e0" }],
    build: (f) => {
      if (f.auto) f.reference = null;
      delete f.auto;
    },
  },
  { kind: "denoise", label: "노이즈 감소", params: [{ key: "strength", label: "", min: 1, max: 3, def: 1 }] },
  {
    kind: "bloom",
    label: "뽀샤시",
    params: [
      { key: "radius", label: "블러", min: 1, max: 40, def: 10 },
      { key: "amount", label: "알파", min: 0, max: 100, def: 50 },
    ],
    checks: [{ key: "shiny", label: "화사하게 (끄면 부드럽게)", def: false }],
  },
  {
    kind: "cartoon",
    label: "카툰 효과",
    params: [
      { key: "edges", label: "윤곽선", min: 0, max: 100, def: 40 },
      { key: "levels", label: "색 단계", min: 2, max: 16, def: 6 },
    ],
  },
  { kind: "print", label: "판화 효과", params: [{ key: "threshold", label: "기준 밝기", min: 0, max: 100, def: 50 }] },
  {
    kind: "ascii",
    label: "아스키 아트",
    params: [{ key: "cell", label: "글자 크기", min: 4, max: 32, def: 10 }],
    checks: [{ key: "color", label: "글자를 원래 색으로", def: false }],
  },
  {
    kind: "afterimage",
    label: "잔상 효과",
    params: [{ key: "strength", label: "", min: 0, max: 95, def: 60 }],
    hint: "앞 프레임이 겹쳐 남습니다 (미리보기는 앞 12장으로 계산).",
  },
  {
    kind: "chromaKey",
    label: "크로마키",
    params: [{ key: "range", label: "색상 범위", min: 0, max: 100, def: 30 }],
    colors: [
      { key: "color", label: "크로마키 색상", def: "#00ff00" },
      { key: "replace", label: "대치할 색상", def: "#ffffff" },
    ],
    checks: [
      { key: "adjacent", label: "인접한 영역만 선택", def: false },
      { key: "transparent", label: "투명하게 (끄면 대치할 색상으로)", def: true },
    ],
    build: (f) => {
      if (f.transparent) f.replace = null;
      delete f.transparent;
    },
  },
  { kind: "eraser", label: "지우개", region: true, regionRequired: true, hint: "지울 곳을 미리보기에서 드래그하세요." },
];

/** 필터 분류 (긴 목록을 나눠 보여 준다) */
type Group = "basic" | "tone" | "blur" | "fx";
const GROUPS: [Group, string][] = [
  ["basic", "기본 보정"],
  ["tone", "색감"],
  ["blur", "흐림 · 가리기"],
  ["fx", "특수 효과"],
];
const GROUP_OF: Record<Kind, Group> = {
  brightness: "basic",
  contrast: "basic",
  saturation: "basic",
  temperature: "basic",
  gamma: "basic",
  sharpen: "basic",
  denoise: "basic",
  whiteBalance: "basic",
  curve: "basic",
  channels: "basic",
  grayscale: "tone",
  sepia: "tone",
  invert: "tone",
  monotone: "tone",
  duotone: "tone",
  blur: "blur",
  mosaic: "blur",
  irisBlur: "blur",
  tiltShift: "blur",
  vignette: "blur",
  eraser: "blur",
  bloom: "fx",
  cartoon: "fx",
  print: "fx",
  ascii: "fx",
  scanline: "fx",
  afterimage: "fx",
  chromaKey: "fx",
};
/** 자주 쓰는 보정: 기본 보정 맨 위에 막대로 바로 조절 (0이면 꺼짐) */
const QUICK: Kind[] = ["brightness", "contrast", "saturation", "temperature"];

interface State {
  on: Partial<Record<Kind, boolean>>;
  values: Record<string, number | string>;
  regionMode: "all" | "rect" | "ellipse";
}

const PRESET_KEY = "opencam.filterPresets";

function initialState(): State {
  const values: State["values"] = {};
  for (const d of DEFS) {
    d.params?.forEach((p) => (values[`${d.kind}.${p.key}`] = p.def));
    d.colors?.forEach((c) => (values[`${d.kind}.${c.key}`] = c.def));
    d.checks?.forEach((c) => (values[`${d.kind}.${c.key}`] = c.def ? 1 : 0));
  }
  for (const k of QUICK) values[`${k}.value`] = 0;
  return { on: {}, values, regionMode: "all" };
}

function loadPresets(): Record<string, State> {
  try {
    return JSON.parse(localStorage.getItem(PRESET_KEY) ?? "{}");
  } catch {
    return {};
  }
}

function buildFilters(st: State, region: Region | null): Filter[] {
  const out: Filter[] = [];
  for (const d of DEFS) {
    if (!st.on[d.kind]) continue;
    const f: Record<string, unknown> = { kind: d.kind };
    d.params?.forEach((p) => (f[p.key] = Number(st.values[`${d.kind}.${p.key}`])));
    d.colors?.forEach((c) => (f[c.key] = hexToRgb(String(st.values[`${d.kind}.${c.key}`]))));
    d.checks?.forEach((c) => (f[c.key] = Number(st.values[`${d.kind}.${c.key}`] ?? (c.def ? 1 : 0)) === 1));
    if (d.region) f.region = st.regionMode === "all" && !d.regionRequired ? null : region;
    d.build?.(f);
    out.push(f as unknown as Filter);
  }
  return out;
}

interface Props {
  frame: FrameInfo;
  scale: number;
  /** target: 필터를 쓸 프레임 번호 (빈 목록이면 전체) */
  onApply: (filters: Filter[], target: number[]) => void;
  onCancel: () => void;
  onError: (m: string) => void;
}

export default function FilterView({ frame, scale, onApply, onCancel, onError }: Props) {
  const frames = useContext(FrameContext);
  const [st, setSt] = useState<State>(initialState);
  const [group, setGroup] = useState<Group>("basic");
  const [region, setRegion] = useState<Region>(() => ({
    x: Math.round(frame.width * 0.3),
    y: Math.round(frame.height * 0.3),
    w: Math.round(frame.width * 0.4),
    h: Math.round(frame.height * 0.4),
    ellipse: false,
  }));
  const [preview, setPreview] = useState<string | null>(null);
  const [showOriginal, setShowOriginal] = useState(false);
  const [scope, setScope] = useState<Scope>(() => defaultScope(frames));
  const [presets, setPresets] = useState(loadPresets);
  const drag = useRef<{ ax: number; ay: number } | null>(null);
  const box = useRef<HTMLDivElement>(null);

  const reg: Region = { ...region, ellipse: st.regionMode === "ellipse" };
  const filters = buildFilters(st, reg);
  const key = JSON.stringify(filters);
  useReportDirty(filters.length > 0);
  const usesRegion =
    (st.regionMode !== "all" && DEFS.some((d) => d.region && st.on[d.kind])) ||
    DEFS.some((d) => d.regionRequired && st.on[d.kind]);

  // 필터가 바뀌면 잠깐 기다렸다가 미리보기를 새로 만든다
  useEffect(() => {
    if (filters.length === 0) {
      setPreview(null);
      return;
    }
    let url: string | null = null;
    let alive = true;
    const t = window.setTimeout(async () => {
      try {
        const buf = await api.filterPreview(frame.id, filters);
        if (!alive) return;
        url = URL.createObjectURL(new Blob([buf], { type: "image/png" }));
        setPreview(url);
      } catch (e) {
        onError(errText(e));
      }
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
      if (url) window.setTimeout(() => URL.revokeObjectURL(url!), 2000);
    };
  }, [key, frame.id]);

  useEffect(() => {
    const toImage = (e: MouseEvent) => {
      const r = box.current!.getBoundingClientRect();
      return {
        x: Math.max(0, Math.min(frame.width, (e.clientX - r.left) / scale)),
        y: Math.max(0, Math.min(frame.height, (e.clientY - r.top) / scale)),
      };
    };
    const onMove = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      const p = toImage(e);
      setRegion((r) => ({
        ...r,
        x: Math.round(Math.min(d.ax, p.x)),
        y: Math.round(Math.min(d.ay, p.y)),
        w: Math.max(1, Math.round(Math.abs(p.x - d.ax))),
        h: Math.max(1, Math.round(Math.abs(p.y - d.ay))),
      }));
    };
    const onUp = () => (drag.current = null);
    const onKey = (e: KeyboardEvent) => {
      // 위에 확인·설정 같은 대화상자가 떠 있으면 그 창의 키다 (Esc·Enter가 도구까지 닫거나 적용하지 않게)
      if (isModalOpen()) return;
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("keydown", onKey);
    };
  }, [frame, scale, onCancel]);

  const setOn = (k: Kind, v: boolean) => setSt((s) => ({ ...s, on: { ...s.on, [k]: v } }));
  const setVal = (k: string, v: number | string) => setSt((s) => ({ ...s, values: { ...s.values, [k]: v } }));
  /** 빠른 보정: 0이 아니면 켜진 것 */
  const setQuick = (k: Kind, v: number) =>
    setSt((s) => ({ ...s, values: { ...s.values, [`${k}.value`]: v }, on: { ...s.on, [k]: v !== 0 } }));

  const savePreset = async () => {
    const name = await promptAsk("필터 설정 이름");
    if (!name) return;
    const next = { ...presets, [name]: st };
    setPresets(next);
    try {
      localStorage.setItem(PRESET_KEY, JSON.stringify(next));
    } catch {
      onError("설정을 저장하지 못했습니다");
    }
  };

  const W = frame.width;
  const H = frame.height;
  const onNames = DEFS.filter((d) => st.on[d.kind]).map((d) => d.label);
  const countIn = (g: Group) => DEFS.filter((d) => GROUP_OF[d.kind] === g && st.on[d.kind]).length;
  const why = filters.length === 0 ? "필터를 하나 이상 켜세요" : null;

  /** 필터 하나의 세부 값 (켰을 때만) */
  const detail = (d: Def) => (
    <div className="pfx-body">
      {d.params?.map((p) => {
        const k = `${d.kind}.${p.key}`;
        return (
          <Field key={k} label={p.label || "세기"}>
            <Slider
              value={Number(st.values[k])}
              min={p.min}
              max={p.max}
              step={p.step}
              unit={p.unit}
              label={`${d.label} ${p.label || "세기"}`}
              onChange={(v) => setVal(k, v)}
            />
          </Field>
        );
      })}
      {d.colors?.map((c) => {
        const k = `${d.kind}.${c.key}`;
        return (
          <Field key={k} label={c.label}>
            <ColorInput label={`${d.label} ${c.label}`} value={String(st.values[k])} onChange={(v) => setVal(k, v)} />
          </Field>
        );
      })}
      {d.checks?.map((c) => {
        const k = `${d.kind}.${c.key}`;
        return <Check key={k} label={c.label} checked={Number(st.values[k]) === 1} onChange={(v) => setVal(k, v ? 1 : 0)} />;
      })}
      {d.region && !d.regionRequired && (
        <Field label="영역">
          <select
            aria-label="필터를 쓸 영역"
            value={st.regionMode}
            onChange={(e) => setSt((s) => ({ ...s, regionMode: e.target.value as State["regionMode"] }))}
          >
            <option value="all">전체 화면</option>
            <option value="rect">사각형 (미리보기에서 드래그)</option>
            <option value="ellipse">타원 (미리보기에서 드래그)</option>
          </select>
        </Field>
      )}
      {d.hint && <p className="hint">{d.hint}</p>}
    </div>
  );

  return (
    <div className="tool">
      <div className="tool-area">
        <div
          className="tool-box"
          ref={box}
          style={{ width: W * scale, height: H * scale, cursor: usesRegion ? "crosshair" : "default" }}
          onMouseDown={(e) => {
            if (!usesRegion || e.button !== 0) return;
            e.preventDefault();
            const r = box.current!.getBoundingClientRect();
            drag.current = { ax: (e.clientX - r.left) / scale, ay: (e.clientY - r.top) / scale };
          }}
        >
          <img
            src={showOriginal || !preview ? frameUrl(frame.id) : preview}
            width={W * scale}
            height={H * scale}
            draggable={false}
          />
          {usesRegion && (
            <div
              className={`region ${st.regionMode === "ellipse" ? "ellipse" : ""}`}
              style={{ left: region.x * scale, top: region.y * scale, width: region.w * scale, height: region.h * scale }}
            />
          )}
        </div>
      </div>

      <SidePanel
        title="필터"
        desc={`여러 필터를 함께 켤 수 있습니다. 미리보기는 지금 프레임(${frames.current + 1}번)으로 보여 줍니다.`}
        foot={
          <PanelFoot
            scope={<ScopePicker value={scope} onChange={setScope} span={false} />}
            summary={onNames.length > 0 && `켠 필터 ${onNames.length}개: ${onNames.join(", ")}`}
            disabledReason={why}
            onCancel={onCancel}
            onApply={() => onApply(filters, scopeSel(scope, frames))}
          />
        }
      >
        <Section
          title="필터"
          action={
            <button
              disabled={filters.length === 0}
              title={filters.length === 0 ? "켠 필터가 없습니다" : "누르고 있는 동안 원본을 보여 줍니다"}
              onMouseDown={() => setShowOriginal(true)}
              onMouseUp={() => setShowOriginal(false)}
              onMouseLeave={() => setShowOriginal(false)}
            >
              <IconEye size={16} />
              원본 보기
            </button>
          }
        >
          <div className="ptabs" role="tablist" aria-label="필터 분류">
            {GROUPS.map(([g, label]) => (
              <button key={g} role="tab" aria-selected={group === g} className={group === g ? "on" : ""} onClick={() => setGroup(g)}>
                {label}
                {countIn(g) > 0 && <b>{countIn(g)}</b>}
              </button>
            ))}
          </div>
          {group === "basic" && (
            <div className="pquick">
              {QUICK.map((k) => {
                const d = DEFS.find((x) => x.kind === k)!;
                return (
                  <Field key={k} label={d.label}>
                    <Slider
                      value={Number(st.values[`${k}.value`])}
                      min={-100}
                      max={100}
                      label={d.label}
                      onChange={(v) => setQuick(k, v)}
                    />
                  </Field>
                );
              })}
              <p className="hint">0이면 꺼집니다.</p>
            </div>
          )}
          <div className="pfx-list">
            {DEFS.filter((d) => GROUP_OF[d.kind] === group && !QUICK.includes(d.kind)).map((d) => (
              <div key={d.kind} className={`pfx ${st.on[d.kind] ? "on" : ""}`}>
                <div className="pfx-head">
                  <Check label={d.label} checked={!!st.on[d.kind]} onChange={(v) => setOn(d.kind, v)} />
                </div>
                {st.on[d.kind] && detail(d)}
              </div>
            ))}
          </div>
        </Section>

        <Section title="설정 저장" collapsible defaultOpen={false} summary={`저장한 설정 ${Object.keys(presets).length}개`}>
          <div className="row">
            <button onClick={savePreset} disabled={filters.length === 0} title={filters.length === 0 ? "켠 필터가 없습니다" : undefined}>
              지금 설정 저장…
            </button>
            <button onClick={() => setSt(initialState())} disabled={filters.length === 0}>
              모두 끄기
            </button>
          </div>
          <Field label="불러오기">
            <select
              aria-label="저장한 설정 불러오기"
              value=""
              disabled={Object.keys(presets).length === 0}
              onChange={(e) => {
                const p = presets[e.target.value];
                if (p) setSt(p);
              }}
            >
              <option value="">{Object.keys(presets).length ? "저장한 설정 고르기" : "저장한 설정 없음"}</option>
              {Object.keys(presets).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </Field>
        </Section>
      </SidePanel>
    </div>
  );
}
