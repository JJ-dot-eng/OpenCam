// 삽입 화면 (B): 텍스트·이미지·스티커·말풍선을 여러 개 올리고(레이어), 등장/퇴장 애니메이션과 함께 덧씌운다.
// 그림은 여기(브라우저 캔버스)에서 만들어 Rust로 보낸다. 그래서 윈도우 글꼴·이모지를 그대로 쓸 수 있다.

import { ComponentType, useContext, useEffect, useMemo, useRef, useState } from "react";
import {
  IconAlignCenter,
  IconAlignLeft,
  IconAlignRight,
  IconArrowDown,
  IconArrowUp,
  IconBold,
  IconChevronDown,
  IconClipboard,
  IconItalic,
  IconMessageCircle,
  IconMoodSmile,
  IconPhoto,
  IconPlus,
  IconTrash,
  IconTypography,
} from "@tabler/icons-react";
import { Anim, api, errText, FrameInfo, frameUrl, OverlayMeta } from "./api";
import { Check, isModalOpen } from "./dialogs";
import SidePanel from "./SidePanel";
import {
  ColorInput,
  defaultScope,
  Field,
  FrameContext,
  NumInput,
  PanelFoot,
  Scope,
  ScopePicker,
  scopeSel,
  Section,
  Segmented,
  Slider,
  useReportDirty,
} from "./Panel";

type Icon = ComponentType<{ size?: number }>;

const FONTS = ["맑은 고딕", "굴림", "돋움", "바탕", "궁서", "Arial", "Impact", "Segoe UI", "Times New Roman", "Comic Sans MS"];

/** 등장 10종·퇴장 8종 */
const ANIMS_IN: [Anim, string][] = [
  ["none", "효과 없음"],
  ["zoomIn", "서서히 커짐"],
  ["fade", "서서히 나타남"],
  ["zoomOut", "작아지며 서서히 나타남"],
  ["rotateBig", "회전(작아지며 나타남)"],
  ["rotateSmall", "회전(커지며 나타남)"],
  ["left", "좌측에서 등장"],
  ["right", "우측에서 등장"],
  ["top", "위에서 내려옴"],
  ["bottom", "아래에서 올라옴"],
];
const ANIMS_OUT: [Anim, string][] = [
  ["none", "효과 없음"],
  ["zoomIn", "서서히 작아짐"],
  ["fade", "서서히 사라짐"],
  ["zoomOut", "커지며 사라짐"],
  ["fadeSmall", "작아지며 사라짐"],
  ["left", "왼쪽으로 나감"],
  ["right", "오른쪽으로 나감"],
  ["bottom", "아래쪽으로 내려감"],
  ["top", "위로 올라감"],
];

const STICKERS = [
  "😀", "😂", "😍", "🥰", "😎", "🤔", "😮", "😢", "😡", "🥳", "👍", "👏", "🙏", "💪", "👀", "❤️", "🔥", "⭐",
  "✨", "🎉", "💯", "✅", "❌", "⚠️", "❓", "❗", "💡", "📌", "➡️", "⬅️", "⬆️", "⬇️", "🔴", "🟢", "🔵", "👆",
];

type Kind = "text" | "image" | "sticker" | "bubble";
type BubbleShape = "round" | "ellipse" | "thought" | "shout";
type Tail = "none" | "bl" | "br" | "left" | "right";

interface TextStyle {
  font: string;
  size: number;
  bold: boolean;
  italic: boolean;
  color: string;
  /** 외곽선을 쓰나 (꺼도 두께는 기억한다) */
  outlineOn: boolean;
  outline: number;
  outlineColor: string;
  shadow: boolean;
  align: CanvasTextAlign;
}

interface Item {
  id: number;
  kind: Kind;
  /** 텍스트·말풍선 글자, 스티커 이모지 */
  text: string;
  style: TextStyle;
  image: HTMLImageElement | null;
  /** 이미지·스티커 크기 % */
  scale: number;
  rotate: number;
  cx: number;
  cy: number;
  animIn: Anim;
  animOut: Anim;
  inMs: number;
  outMs: number;
  shape: BubbleShape;
  tail: Tail;
  fill: string;
  stroke: string;
  strokeW: number;
  noText: boolean;
}

export interface InsertChunk {
  rgba: Uint8Array;
  meta: Omit<OverlayMeta, "sel">;
}

const fontOf = (s: TextStyle) => `${s.italic ? "italic " : ""}${s.bold ? "bold " : ""}${s.size}px "${s.font}"`;

/** 여러 줄 글자를 그린 캔버스 (외곽선·그림자 포함) */
function renderText(text: string, p: TextStyle, withEffects = true): HTMLCanvasElement {
  const lines = text.split("\n");
  const m = document.createElement("canvas").getContext("2d")!;
  m.font = fontOf(p);
  const lineH = Math.round(p.size * 1.25);
  const textW = Math.max(1, ...lines.map((l) => Math.ceil(m.measureText(l).width)));
  const outline = withEffects && p.outlineOn ? p.outline : 0;
  const shadow = withEffects && p.shadow;
  const pad = Math.ceil(outline + (shadow ? p.size / 6 + 4 : 0) + 4);
  const c = document.createElement("canvas");
  c.width = textW + pad * 2;
  c.height = lineH * lines.length + pad * 2;
  const ctx = c.getContext("2d")!;
  ctx.font = fontOf(p);
  ctx.textBaseline = "middle";
  ctx.textAlign = p.align;
  ctx.lineJoin = "round";
  const x = p.align === "left" ? pad : p.align === "right" ? c.width - pad : c.width / 2;
  lines.forEach((line, i) => {
    const y = pad + lineH * i + lineH / 2;
    ctx.save();
    if (shadow) {
      ctx.shadowColor = "rgba(0,0,0,0.55)";
      ctx.shadowBlur = p.size / 8;
      ctx.shadowOffsetX = p.size / 14 + 1;
      ctx.shadowOffsetY = p.size / 14 + 1;
    }
    if (outline > 0) {
      ctx.strokeStyle = p.outlineColor;
      ctx.lineWidth = outline * 2;
      ctx.strokeText(line, x, y);
      ctx.shadowColor = "transparent";
    }
    ctx.fillStyle = p.color;
    ctx.fillText(line, x, y);
    ctx.restore();
  });
  return c;
}

/** 말풍선: 모양 + 꼬리 + (있으면) 글자 */
function renderBubble(it: Item): HTMLCanvasElement {
  const text = it.noText ? null : renderText(it.text || " ", it.style, false);
  const pad = Math.round(it.style.size * 0.6);
  let bw = (text ? text.width : it.style.size * 4) + pad * 2;
  let bh = (text ? text.height : it.style.size * 2) + pad * 2;
  if (it.shape !== "round") {
    // 둥근 모양은 글자가 모서리에 걸리지 않게 더 크게
    bw *= 1.3;
    bh *= 1.35;
  }
  const tailLen = it.tail === "none" ? 0 : Math.round(Math.min(bw, bh) * 0.45);
  const sw = it.strokeW;
  const m = sw + 2;
  const left = it.tail === "left" ? tailLen : 0;
  const right = it.tail === "right" ? tailLen : 0;
  const below = it.tail === "bl" || it.tail === "br" ? tailLen : 0;
  const c = document.createElement("canvas");
  c.width = Math.ceil(bw + left + right + m * 2);
  c.height = Math.ceil(bh + below + m * 2);
  const ctx = c.getContext("2d")!;
  const x0 = m + left;
  const y0 = m;
  const cx = x0 + bw / 2;
  const cy = y0 + bh / 2;
  ctx.lineJoin = "round";
  ctx.fillStyle = it.fill;
  ctx.strokeStyle = it.stroke;
  ctx.lineWidth = sw;

  const body = new Path2D();
  if (it.shape === "round") {
    body.roundRect(x0, y0, bw, bh, Math.min(bw, bh) * 0.3);
  } else if (it.shape === "shout") {
    // 뾰족뾰족한 외침 말풍선
    const spikes = 18;
    for (let k = 0; k <= spikes * 2; k++) {
      const a = (k / (spikes * 2)) * Math.PI * 2;
      const r = k % 2 === 0 ? 1 : 0.82;
      const px = cx + Math.cos(a) * (bw / 2) * r;
      const py = cy + Math.sin(a) * (bh / 2) * r;
      if (k === 0) body.moveTo(px, py);
      else body.lineTo(px, py);
    }
    body.closePath();
  } else {
    body.ellipse(cx, cy, bw / 2, bh / 2, 0, 0, Math.PI * 2);
  }

  // 꼬리 (생각 풍선은 작은 동그라미들)
  const tail = new Path2D();
  if (it.tail !== "none") {
    if (it.shape === "thought") {
      const dirX = it.tail === "left" ? -1 : it.tail === "right" ? 1 : it.tail === "bl" ? -0.6 : 0.6;
      const dirY = it.tail === "bl" || it.tail === "br" ? 1 : 0.3;
      const sx = cx + dirX * (bw / 2) * 0.8;
      const sy = cy + dirY * (bh / 2) * 0.85;
      [0.35, 0.65, 0.95].forEach((t, k) => {
        const r = Math.max(2, tailLen * (0.22 - k * 0.06));
        const px = sx + dirX * tailLen * t;
        const py = sy + dirY * tailLen * t;
        tail.moveTo(px + r, py);
        tail.arc(px, py, r, 0, Math.PI * 2);
      });
    } else {
      const w2 = Math.min(bw, bh) * 0.18;
      if (it.tail === "bl" || it.tail === "br") {
        const bx = it.tail === "bl" ? x0 + bw * 0.3 : x0 + bw * 0.7;
        const tipX = it.tail === "bl" ? bx - tailLen * 0.6 : bx + tailLen * 0.6;
        tail.moveTo(bx - w2, y0 + bh * 0.85);
        tail.lineTo(tipX, y0 + bh + tailLen);
        tail.lineTo(bx + w2, y0 + bh * 0.85);
      } else {
        const sx = it.tail === "left" ? x0 + bw * 0.15 : x0 + bw * 0.85;
        const tipX = it.tail === "left" ? m : x0 + bw + tailLen;
        tail.moveTo(sx, cy - w2);
        tail.lineTo(tipX, cy + bh * 0.25);
        tail.lineTo(sx, cy + w2);
      }
      tail.closePath();
    }
  }
  // 테두리 → 채우기 순서로 그려서 꼬리와 몸통 이음새의 선을 감춘다
  if (sw > 0) {
    ctx.stroke(tail);
    ctx.stroke(body);
  }
  ctx.fill(tail);
  ctx.fill(body);
  if (text) ctx.drawImage(text, cx - text.width / 2, cy - text.height / 2);
  return c;
}

/** 크기·회전을 적용한 새 캔버스 (회전하면 커진 테두리 상자 크기) */
function transform(src: CanvasImageSource, sw: number, sh: number, scalePct: number, deg: number): HTMLCanvasElement {
  const s = scalePct / 100;
  const w = sw * s;
  const h = sh * s;
  const rad = (deg * Math.PI) / 180;
  const cos = Math.abs(Math.cos(rad));
  const sin = Math.abs(Math.sin(rad));
  const c = document.createElement("canvas");
  c.width = Math.max(1, Math.ceil(w * cos + h * sin));
  c.height = Math.max(1, Math.ceil(w * sin + h * cos));
  const ctx = c.getContext("2d")!;
  ctx.translate(c.width / 2, c.height / 2);
  ctx.rotate(rad);
  ctx.drawImage(src, -w / 2, -h / 2, w, h);
  return c;
}

function render(it: Item): HTMLCanvasElement | null {
  if (it.kind === "image") {
    if (!it.image) return null;
    return transform(it.image, it.image.naturalWidth, it.image.naturalHeight, it.scale, it.rotate);
  }
  if (it.kind === "text") {
    if (!it.text.trim()) return null;
    const c = renderText(it.text, it.style);
    return transform(c, c.width, c.height, 100, it.rotate);
  }
  if (it.kind === "sticker") {
    const c = renderText(it.text, { ...it.style, font: "Segoe UI Emoji", outlineOn: false, bold: false, italic: false }, true);
    return transform(c, c.width, c.height, it.scale, it.rotate);
  }
  const c = renderBubble(it);
  return transform(c, c.width, c.height, 100, it.rotate);
}

const KIND_LABEL: Record<Kind, string> = { text: "텍스트", image: "이미지", sticker: "스티커", bubble: "말풍선" };
const KIND_ICON: Record<Kind, Icon> = { text: IconTypography, image: IconPhoto, sticker: IconMoodSmile, bubble: IconMessageCircle };

interface Props {
  frame: FrameInfo;
  scale: number;
  /** target: 덧씌울 프레임 번호 (빈 목록이면 전체, 여러 장이면 처음~끝 구간) */
  onApply: (chunks: InsertChunk[], target: number[]) => void;
  onCancel: () => void;
  onError: (m: string) => void;
}

export default function InsertView({ frame, scale, onApply, onCancel, onError }: Props) {
  const W = frame.width;
  const H = frame.height;
  const frames = useContext(FrameContext);
  const nextId = useRef(1);
  const newItem = (kind: Kind, extra: Partial<Item> = {}): Item => ({
    id: nextId.current++,
    kind,
    text: kind === "bubble" ? "말풍선" : kind === "sticker" ? "👍" : "텍스트",
    style: {
      font: FONTS[0],
      size: Math.max(16, Math.round(H / (kind === "sticker" ? 5 : 10))),
      bold: kind !== "bubble",
      italic: false,
      color: kind === "bubble" ? "#222222" : "#ffffff",
      outlineOn: kind === "text",
      outline: 3,
      outlineColor: "#000000",
      shadow: kind === "text",
      align: "center",
    },
    image: null,
    scale: 100,
    rotate: 0,
    cx: W / 2,
    cy: kind === "text" ? H * 0.8 : H / 2,
    animIn: "fade",
    animOut: "fade",
    inMs: 300,
    outMs: 300,
    shape: "round",
    tail: "bl",
    fill: "#ffffff",
    stroke: "#222222",
    strokeW: 3,
    noText: false,
    ...extra,
  });
  const [items, setItems] = useState<Item[]>(() => [newItem("text")]);
  const [selId, setSelId] = useState<number>(1);
  const [scope, setScope] = useState<Scope>(() => defaultScope(frames));
  const [darken, setDarken] = useState(true);
  // 처음 넣어 둔 텍스트를 그대로 두었으면 바꾼 것이 없는 것으로 본다
  const [touched, setTouched] = useState(false);
  useReportDirty(touched);
  const drag = useRef<{ id: number; dx: number; dy: number } | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const sel = items.find((i) => i.id === selId) ?? null;
  const rendered = useMemo(() => new Map(items.map((it) => [it.id, render(it)])), [items]);

  const update = (patch: Partial<Item>) => {
    setTouched(true);
    setItems((all) => all.map((i) => (i.id === selId ? { ...i, ...patch } : i)));
  };
  const setStyle = <K extends keyof TextStyle>(k: K, v: TextStyle[K]) => {
    if (sel) update({ style: { ...sel.style, [k]: v } });
  };
  const add = (it: Item) => {
    setTouched(true);
    setItems((all) => [...all, it]);
    setSelId(it.id);
  };
  const remove = () => {
    setTouched(true);
    setItems((all) => all.filter((i) => i.id !== selId));
    setSelId(items.find((i) => i.id !== selId)?.id ?? 0);
  };
  /** 순서: 목록 뒤의 것이 위(앞)에 그려진다. d=1이면 한 칸 앞으로 */
  const move = (d: number) => {
    setTouched(true);
    setItems((all) => {
      const i = all.findIndex((x) => x.id === selId);
      const j = i + d;
      if (i < 0 || j < 0 || j >= all.length) return all;
      const n = [...all];
      [n[i], n[j]] = [n[j], n[i]];
      return n;
    });
  };

  const loadImage = (src: string) => {
    const img = new Image();
    img.onload = () => {
      const fit = Math.max(5, Math.min(100, Math.floor(Math.min(W / img.naturalWidth, H / img.naturalHeight) * 100)));
      add(newItem("image", { image: img, scale: fit }));
    };
    img.onerror = () => onError("이미지를 열 수 없습니다");
    img.src = src;
  };
  const pasteImage = async () => {
    try {
      const buf = await api.clipboardImagePng();
      loadImage(URL.createObjectURL(new Blob([buf], { type: "image/png" })));
    } catch (e) {
      onError(errText(e));
    }
  };

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      const r = box.current!.getBoundingClientRect();
      const x = (e.clientX - r.left) / scale - d.dx;
      const y = (e.clientY - r.top) / scale - d.dy;
      setTouched(true);
      setItems((all) => all.map((i) => (i.id === d.id ? { ...i, cx: x, cy: y } : i)));
    };
    const onUp = () => (drag.current = null);
    const onKey = (e: KeyboardEvent) => {
      // 위에 확인·설정 같은 대화상자가 떠 있으면 그 창의 키다 (Esc가 도구까지 닫지 않게)
      if (isModalOpen()) return;
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
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
  }, [scale, onCancel]);

  const chunks = (): InsertChunk[] => {
    const out: InsertChunk[] = [];
    for (const it of items) {
      const c = rendered.get(it.id);
      if (!c) continue;
      const data = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
      out.push({
        rgba: new Uint8Array(data.buffer, data.byteOffset, data.byteLength),
        meta: {
          w: c.width,
          h: c.height,
          x: it.cx - c.width / 2,
          y: it.cy - c.height / 2,
          animIn: it.animIn,
          animOut: it.animOut,
          inMs: it.inMs,
          outMs: it.outMs,
        },
      });
    }
    return out;
  };
  const visible = items.filter((it) => rendered.get(it.id)).length;
  const why =
    items.length === 0 ? "넣을 개체가 없습니다. 위의 '추가'로 넣으세요" : visible === 0 ? "글자가 비어 있어 넣을 것이 없습니다" : null;

  /** 목록에 보일 이름 */
  const nameOf = (it: Item) => {
    if (it.kind === "image") return `이미지 ${items.filter((x) => x.kind === "image" && x.id <= it.id).length}`;
    if (it.kind === "sticker") return `스티커 ${it.text}`;
    if (it.kind === "bubble" && it.noText) return "말풍선 (글자 없음)";
    return it.text.split("\n")[0].trim() || `${KIND_LABEL[it.kind]} (비어 있음)`;
  };
  const selIndex = sel ? items.indexOf(sel) : -1;
  const isText = sel && (sel.kind === "text" || sel.kind === "bubble");
  const sized = sel && (sel.kind === "image" || sel.kind === "sticker");
  const place = (where: "top" | "middle" | "bottom") => {
    if (!sel) return;
    const h = rendered.get(sel.id)?.height ?? 0;
    const cy = where === "middle" ? H / 2 : where === "top" ? h / 2 + H * 0.04 : H - h / 2 - H * 0.04;
    update({ cx: W / 2, cy });
  };
  const animName = (list: [Anim, string][], a: Anim) => list.find(([k]) => k === a)?.[1] ?? "";

  return (
    <div className="tool">
      <div className="tool-area">
        <div
          className={`tool-box ${darken ? "show-outside" : ""}`}
          ref={box}
          style={{ width: W * scale, height: H * scale }}
          onMouseDown={() => setSelId(0)}
        >
          <img src={frameUrl(frame.id)} width={W * scale} height={H * scale} draggable={false} />
          {darken && <div className="darken-outside" />}
          {items.map((it) => {
            const c = rendered.get(it.id);
            if (!c) return null;
            return (
              <CanvasItem
                key={it.id}
                canvas={c}
                selected={it.id === selId}
                style={{ left: (it.cx - c.width / 2) * scale, top: (it.cy - c.height / 2) * scale }}
                scale={scale}
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
                  e.preventDefault();
                  e.stopPropagation();
                  setSelId(it.id);
                  const r = box.current!.getBoundingClientRect();
                  drag.current = { id: it.id, dx: (e.clientX - r.left) / scale - it.cx, dy: (e.clientY - r.top) / scale - it.cy };
                }}
              />
            );
          })}
        </div>
      </div>

      <SidePanel
        title="텍스트 · 이미지"
        desc="미리보기에서 개체를 끌어 옮길 수 있습니다."
        foot={
          <PanelFoot
            scope={<ScopePicker value={scope} onChange={setScope} span />}
            summary={visible > 0 && `덧씌울 개체 ${visible}개`}
            disabledReason={why}
            onCancel={onCancel}
            onApply={() => onApply(chunks(), scopeSel(scope, frames))}
          />
        }
      >
        <Section
          title="개체"
          action={
            <AddMenu
              items={[
                ["텍스트", IconTypography, () => add(newItem("text"))],
                ["말풍선", IconMessageCircle, () => add(newItem("bubble"))],
                ["스티커", IconMoodSmile, () => add(newItem("sticker"))],
                ["이미지 파일…", IconPhoto, () => fileRef.current?.click()],
                ["클립보드 이미지", IconClipboard, pasteImage],
              ]}
            />
          }
        >
          <input
            ref={fileRef}
            type="file"
            hidden
            accept="image/png,image/jpeg,image/gif,image/webp,image/bmp"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) loadImage(URL.createObjectURL(file));
              e.target.value = "";
            }}
          />
          {items.length === 0 ? (
            <p className="hint">개체가 없습니다. 오른쪽 위 "추가"로 넣으세요.</p>
          ) : (
            <div className="player-list" role="listbox" aria-label="개체 목록 (위쪽이 앞에 보임)">
              {[...items].reverse().map((it) => {
                const KindIcon = KIND_ICON[it.kind];
                const on = it.id === selId;
                return (
                  <div
                    key={it.id}
                    role="option"
                    aria-selected={on}
                    tabIndex={0}
                    className={`player ${on ? "sel" : ""}`}
                    onClick={() => setSelId(it.id)}
                    onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && setSelId(it.id)}
                  >
                    <KindIcon size={18} />
                    <span className="name">{nameOf(it)}</span>
                    {on && (
                      <span className="acts" onClick={(e) => e.stopPropagation()}>
                        <button
                          className="ghost icon"
                          disabled={selIndex === items.length - 1}
                          title={selIndex === items.length - 1 ? "이미 맨 앞입니다" : "한 칸 앞으로 (다른 개체 위로)"}
                          onClick={() => move(1)}
                        >
                          <IconArrowUp size={16} />
                        </button>
                        <button
                          className="ghost icon"
                          disabled={selIndex === 0}
                          title={selIndex === 0 ? "이미 맨 뒤입니다" : "한 칸 뒤로 (다른 개체 밑으로)"}
                          onClick={() => move(-1)}
                        >
                          <IconArrowDown size={16} />
                        </button>
                        <button className="ghost icon" title="이 개체 삭제" onClick={remove}>
                          <IconTrash size={16} />
                        </button>
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          {items.length > 1 && <p className="hint">목록 위쪽 개체가 앞에 보입니다.</p>}
        </Section>

        {!sel && items.length > 0 && <p className="hint psec">목록이나 미리보기에서 개체를 고르면 설정이 나옵니다.</p>}

        {sel && isText && (
          <Section title="텍스트">
            {!(sel.kind === "bubble" && sel.noText) && (
              <textarea rows={3} aria-label="글자 내용" value={sel.text} onChange={(e) => update({ text: e.target.value })} />
            )}
            <Field label="글꼴">
              <select aria-label="글꼴" value={sel.style.font} onChange={(e) => setStyle("font", e.target.value)}>
                {FONTS.map((f) => (
                  <option key={f} value={f} style={{ fontFamily: f }}>
                    {f}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="글자 크기">
              <NumInput value={sel.style.size} min={6} max={400} unit="px" label="글자 크기" onChange={(v) => v > 0 && setStyle("size", v)} />
              <button
                className={`icon ${sel.style.bold ? "on" : ""}`}
                aria-pressed={sel.style.bold}
                title="굵게"
                onClick={() => setStyle("bold", !sel.style.bold)}
              >
                <IconBold size={18} />
              </button>
              <button
                className={`icon ${sel.style.italic ? "on" : ""}`}
                aria-pressed={sel.style.italic}
                title="기울임"
                onClick={() => setStyle("italic", !sel.style.italic)}
              >
                <IconItalic size={18} />
              </button>
            </Field>
            <Field label="글자 정렬">
              <Segmented
                label="글자 정렬 (여러 줄일 때 줄끼리 맞춤)"
                iconOnly
                value={sel.style.align as "left" | "center" | "right"}
                onChange={(v) => setStyle("align", v)}
                options={[
                  ["left", "왼쪽 정렬", <IconAlignLeft size={18} />],
                  ["center", "가운데 정렬", <IconAlignCenter size={18} />],
                  ["right", "오른쪽 정렬", <IconAlignRight size={18} />],
                ]}
              />
            </Field>
            <p className="hint">글자 정렬은 여러 줄 글자를 줄끼리 맞춥니다. 개체 자리는 "배치 · 회전"에서 바꿉니다.</p>
          </Section>
        )}

        {sel?.kind === "sticker" && (
          <Section title="스티커">
            <div className="stickers">
              {STICKERS.map((s) => (
                <button key={s} className={sel.text === s ? "on" : ""} aria-pressed={sel.text === s} onClick={() => update({ text: s })}>
                  {s}
                </button>
              ))}
            </div>
          </Section>
        )}

        {sel && (sel.kind === "text" || sel.kind === "bubble") && (
          <Section title="모양">
            <Field label="글자색">
              <ColorInput label="글자색" value={sel.style.color} onChange={(v) => setStyle("color", v)} />
            </Field>
            {sel.kind === "text" && (
              <>
                <Field label="외곽선">
                  <input
                    type="checkbox"
                    aria-label="외곽선 쓰기"
                    checked={sel.style.outlineOn}
                    onChange={(e) => setStyle("outlineOn", e.target.checked)}
                  />
                  <ColorInput
                    label="외곽선 색"
                    value={sel.style.outlineColor}
                    disabled={!sel.style.outlineOn}
                    onChange={(v) => setStyle("outlineColor", v)}
                  />
                  <NumInput
                    value={sel.style.outline}
                    min={1}
                    max={30}
                    unit="px"
                    narrow
                    label="외곽선 두께"
                    disabled={!sel.style.outlineOn}
                    onChange={(v) => v >= 1 && setStyle("outline", v)}
                  />
                </Field>
                <Field label="그림자">
                  <Check label="글자 아래 그림자" checked={sel.style.shadow} onChange={(v) => setStyle("shadow", v)} />
                </Field>
              </>
            )}
            {sel.kind === "bubble" && (
              <>
                <Field label="말풍선">
                  <select aria-label="말풍선 모양" value={sel.shape} onChange={(e) => update({ shape: e.target.value as BubbleShape })}>
                    <option value="round">둥근 네모</option>
                    <option value="ellipse">타원</option>
                    <option value="thought">생각</option>
                    <option value="shout">외침</option>
                  </select>
                </Field>
                <Field label="꼬리">
                  <select aria-label="꼬리 방향" value={sel.tail} onChange={(e) => update({ tail: e.target.value as Tail })}>
                    <option value="bl">아래 왼쪽</option>
                    <option value="br">아래 오른쪽</option>
                    <option value="left">왼쪽</option>
                    <option value="right">오른쪽</option>
                    <option value="none">없음</option>
                  </select>
                </Field>
                <Field label="바탕색">
                  <ColorInput label="말풍선 바탕색" value={sel.fill} onChange={(v) => update({ fill: v })} />
                </Field>
                <Field label="테두리">
                  <ColorInput label="말풍선 테두리 색" value={sel.stroke} onChange={(v) => update({ stroke: v })} />
                  <NumInput
                    value={sel.strokeW}
                    min={0}
                    max={20}
                    unit="px"
                    narrow
                    label="테두리 두께 (0이면 없음)"
                    onChange={(v) => v >= 0 && update({ strokeW: v })}
                  />
                </Field>
                <Check label="글자 없이 말풍선만" checked={sel.noText} onChange={(v) => update({ noText: v })} />
              </>
            )}
          </Section>
        )}

        {sel && (
          <Section
            title="배치 · 회전"
            collapsible
            defaultOpen={false}
            summary={`${sized ? `${sel.scale}% · ` : ""}${Math.round(sel.cx)}, ${Math.round(sel.cy)} · ${sel.rotate}°`}
          >
            {sized && (
              <Field label="크기">
                <Slider value={sel.scale} min={5} max={300} unit="%" label="개체 크기" onChange={(v) => update({ scale: v })} />
              </Field>
            )}
            <Field label="회전">
              <Slider value={sel.rotate} min={-180} max={180} unit="°" label="회전" onChange={(v) => update({ rotate: v })} />
            </Field>
            <Field label="가로 위치">
              <NumInput value={Math.round(sel.cx)} unit="px" wide label="가로 위치 (개체 가운데)" onChange={(v) => Number.isFinite(v) && update({ cx: v })} />
            </Field>
            <Field label="세로 위치">
              <NumInput value={Math.round(sel.cy)} unit="px" wide label="세로 위치 (개체 가운데)" onChange={(v) => Number.isFinite(v) && update({ cy: v })} />
            </Field>
            <Field label="개체 자리">
              <button onClick={() => place("top")} title="가로 가운데, 위쪽">
                위
              </button>
              <button onClick={() => place("middle")} title="화면 한가운데">
                가운데
              </button>
              <button onClick={() => place("bottom")} title="가로 가운데, 아래쪽">
                아래
              </button>
            </Field>
          </Section>
        )}

        {sel && (
          <Section
            title="애니메이션"
            collapsible
            defaultOpen={false}
            summary={`${animName(ANIMS_IN, sel.animIn)} · ${animName(ANIMS_OUT, sel.animOut)}`}
          >
            <Field label="등장">
              <select aria-label="등장 효과" value={sel.animIn} onChange={(e) => update({ animIn: e.target.value as Anim })}>
                {ANIMS_IN.map(([a, l]) => (
                  <option key={a} value={a}>
                    {l}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="시간" sub>
              <NumInput
                value={sel.inMs}
                min={0}
                unit="ms"
                wide
                label="등장 시간"
                disabled={sel.animIn === "none"}
                title={sel.animIn === "none" ? "등장 효과를 고르면 정할 수 있습니다" : undefined}
                onChange={(v) => v >= 0 && update({ inMs: v })}
              />
            </Field>
            <Field label="퇴장">
              <select aria-label="퇴장 효과" value={sel.animOut} onChange={(e) => update({ animOut: e.target.value as Anim })}>
                {ANIMS_OUT.map(([a, l]) => (
                  <option key={a} value={a}>
                    {l}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="시간" sub>
              <NumInput
                value={sel.outMs}
                min={0}
                unit="ms"
                wide
                label="퇴장 시간"
                disabled={sel.animOut === "none"}
                title={sel.animOut === "none" ? "퇴장 효과를 고르면 정할 수 있습니다" : undefined}
                onChange={(v) => v >= 0 && update({ outMs: v })}
              />
            </Field>
            <p className="hint">등장은 적용 범위의 처음, 퇴장은 끝에서 일어납니다.</p>
          </Section>
        )}

        <Section title="미리보기">
          <Check label="편집 영역 밖 어둡게 표시" checked={darken} onChange={setDarken} />
        </Section>
      </SidePanel>
    </div>
  );
}

/** "+ 추가" 펼침 메뉴. 바깥을 누르거나 Esc면 닫힌다. */
function AddMenu({ items }: { items: [string, Icon, () => void][] }) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key, true);
    };
  }, [open]);
  return (
    <span className="pmenu-wrap" ref={wrap}>
      <button aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <IconPlus size={16} />
        추가
        <IconChevronDown size={14} />
      </button>
      {open && (
        <div className="pmenu" role="menu">
          {items.map(([label, ItemIcon, run]) => (
            <button
              key={label}
              role="menuitem"
              onClick={() => {
                setOpen(false);
                run();
              }}
            >
              <ItemIcon size={16} />
              {label}
            </button>
          ))}
        </div>
      )}
    </span>
  );
}

/** 만든 캔버스를 화면에 붙여 보여준다 (같은 캔버스를 크기만 줄여서) */
function CanvasItem({
  canvas,
  selected,
  style,
  scale,
  onMouseDown,
}: {
  canvas: HTMLCanvasElement;
  selected: boolean;
  style: React.CSSProperties;
  scale: number;
  onMouseDown: (e: React.MouseEvent) => void;
}) {
  const holder = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    canvas.style.width = `${canvas.width * scale}px`;
    canvas.style.height = `${canvas.height * scale}px`;
    el.replaceChildren(canvas);
  }, [canvas, scale]);
  return (
    <div
      ref={holder}
      className={`insert-item ${selected ? "sel" : ""}`}
      style={style}
      title="드래그해서 옮기기"
      onMouseDown={onMouseDown}
    />
  );
}
