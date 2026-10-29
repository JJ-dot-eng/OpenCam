// 3단계 대화상자: 새 빈 영상, 콜라주, 파일 관리, 프레임 내보내기.

import { useEffect, useMemo, useState } from "react";
import {
  api,
  CollageOptions,
  errText,
  extOf,
  FileEntry,
  formatBytes,
  formatFileDate,
  hexToRgb,
  IMAGE_FILTERS,
  PlayMode,
  Settings,
  VIDEO_EXTS,
} from "./api";
import { Check, Modal, NumField } from "./dialogs";
import { confirmAsk, promptAsk } from "./ask";

// ---------- 새 빈 이미지/영상 ----------

export function NewBlankDialog({
  defaultFps,
  onClose,
  onCreate,
}: {
  defaultFps: number;
  onClose: () => void;
  onCreate: (w: number, h: number, fps: number, sec: number, color: string, transparent: boolean) => void;
}) {
  const [w, setW] = useState(640);
  const [h, setH] = useState(480);
  const [fps, setFps] = useState(defaultFps);
  const [sec, setSec] = useState(3);
  const [color, setColor] = useState("#ffffff");
  const [transparent, setTransparent] = useState(false);
  const valid = w >= 1 && h >= 1 && w <= 8192 && h <= 8192 && fps >= 1 && fps <= 60 && sec > 0 && sec <= 600;
  return (
    <Modal
      title="새 이미지/영상"
      onClose={onClose}
      okDisabled={!valid}
      okText="만들기"
      onOk={() => onCreate(Math.round(w), Math.round(h), fps, sec, color, transparent)}
    >
      <p className="hint">한 색(또는 투명) 배경의 빈 영상을 만듭니다. 텍스트·그리기·스티커로 꾸밀 때 씁니다.</p>
      <div className="row">
        <NumField label="폭" value={w} min={1} max={8192} onChange={setW} />
        <NumField label="높이" value={h} min={1} max={8192} onChange={setH} />
      </div>
      <div className="row">
        <NumField label="초당 프레임 수" value={fps} min={1} max={60} onChange={setFps} />
        <NumField label="총 재생 시간" value={sec} min={0.1} max={600} suffix="초" onChange={setSec} />
      </div>
      <div className="row">
        <label className="field">
          <span>배경 색상</span>
          <input type="color" value={color} disabled={transparent} onChange={(e) => setColor(e.target.value)} />
        </label>
        <Check label="투명한 배경 사용" checked={transparent} onChange={setTransparent} />
      </div>
      <p className="hint">→ {Math.max(1, Math.round(sec * fps))}장</p>
    </Modal>
  );
}

// ---------- 콜라주 (G) ----------

const LAYOUTS: [string, number, number][] = [
  ["가로 2칸", 2, 1],
  ["세로 2칸", 1, 2],
  ["가로 3칸", 3, 1],
  ["세로 3칸", 1, 3],
  ["2 × 2", 2, 2],
  ["3 × 2", 3, 2],
  ["2 × 3", 2, 3],
  ["3 × 3", 3, 3],
];

export function CollageDialog({
  current,
  onClose,
  onMake,
  onError,
}: {
  /** 지금 편집 중인 영상 (없으면 null) */
  current: { width: number; height: number; count: number } | null;
  onClose: () => void;
  onMake: (paths: string[], useCurrent: boolean, bgImage: string | null, o: CollageOptions) => void;
  onError: (m: string) => void;
}) {
  const [paths, setPaths] = useState<string[]>([]);
  const [useCurrent, setUseCurrent] = useState(current !== null);
  const [layout, setLayout] = useState(0);
  const [baseSize, setBaseSize] = useState(current !== null);
  const [width, setWidth] = useState(800);
  const [height, setHeight] = useState(600);
  const [margin, setMargin] = useState(0);
  const [gap, setGap] = useState(10);
  const [roundness, setRoundness] = useState(0);
  const [bg, setBg] = useState("#ffffff");
  const [bgAlpha, setBgAlpha] = useState(255);
  const [bgImage, setBgImage] = useState<string | null>(null);
  const [drawBorder, setDrawBorder] = useState(false);
  const [borderColor, setBorderColor] = useState("#ffffff");
  const [play, setPlay] = useState<PlayMode>("repeat");

  const [, cols, rows] = LAYOUTS[layout];
  // "기본 영상 크기 사용": 지금 영상 한 칸 크기 × 칸 수 + 선·여백
  const canvas = useMemo(() => {
    if (baseSize && current && useCurrent)
      return { w: current.width * cols + gap * (cols - 1) + margin * 2, h: current.height * rows + gap * (rows - 1) + margin * 2 };
    return { w: width, h: height };
  }, [baseSize, current, useCurrent, cols, rows, gap, margin, width, height]);
  const clips = (useCurrent && current ? 1 : 0) + paths.length;
  const valid = clips >= 1 && canvas.w >= 2 && canvas.h >= 2 && canvas.w <= 8192 && canvas.h <= 8192;

  const add = async () => {
    const picked = await api.pickOpenFiles();
    if (picked) setPaths((p) => [...p, ...picked]);
  };
  const move = (i: number, d: number) =>
    setPaths((p) => {
      const j = i + d;
      if (j < 0 || j >= p.length) return p;
      const n = [...p];
      [n[i], n[j]] = [n[j], n[i]];
      return n;
    });

  return (
    <Modal
      title="콜라주"
      onClose={onClose}
      okDisabled={!valid}
      okText="만들기"
      wide
      onOk={() =>
        onMake(paths, useCurrent && current !== null, bgImage, {
          cols,
          rows,
          width: Math.round(canvas.w),
          height: Math.round(canvas.h),
          margin: Math.max(0, Math.round(margin) || 0),
          gap: Math.max(0, Math.round(gap) || 0),
          roundness: Math.max(0, Math.round(roundness) || 0),
          background: hexToRgb(bg),
          backgroundAlpha: Math.max(0, Math.min(255, Math.round(bgAlpha))),
          drawBorder,
          borderColor: hexToRgb(borderColor),
          play,
        })
      }
    >
      <p className="hint">여러 움짤·동영상을 한 화면에 나눠 함께 재생합니다. 동영상은 앞 10초만 씁니다.</p>
      <div className="collage-list">
        {current && (
          <label className="check">
            <input type="checkbox" checked={useCurrent} onChange={(e) => setUseCurrent(e.target.checked)} />
            1. 지금 편집 중인 영상 ({current.width} × {current.height}, {current.count}장)
          </label>
        )}
        {paths.map((p, i) => (
          <div key={i} className="row collage-item">
            <span className="grow">
              {(useCurrent && current ? 2 : 1) + i}. {p.split(/[\\/]/).pop()}
            </span>
            <button onClick={() => move(i, -1)} disabled={i === 0} title="위로">
              ↑
            </button>
            <button onClick={() => move(i, 1)} disabled={i === paths.length - 1} title="아래로">
              ↓
            </button>
            <button onClick={() => setPaths((ps) => ps.filter((_, k) => k !== i))} title="빼기">
              ✕
            </button>
          </div>
        ))}
        <button onClick={add}>+ 파일 추가…</button>
      </div>
      {clips > cols * rows && <p className="warn">칸({cols * rows})보다 영상이 많아 뒤쪽 {clips - cols * rows}개는 빠집니다.</p>}

      <div className="row">
        <label className="field">
          <span>분할 개수</span>
          <select value={layout} onChange={(e) => setLayout(Number(e.target.value))}>
            {LAYOUTS.map(([l], i) => (
              <option key={i} value={i}>
                {l}
              </option>
            ))}
          </select>
        </label>
        {current && useCurrent && <Check label="기본 영상 크기 사용" checked={baseSize} onChange={setBaseSize} />}
      </div>
      <div className="row">
        <NumField label="캔버스 크기" value={canvas.w} min={2} disabled={baseSize && useCurrent && !!current} onChange={setWidth} />
        <span>×</span>
        <NumField label="" value={canvas.h} min={2} disabled={baseSize && useCurrent && !!current} onChange={setHeight} />
      </div>
      <div className="row">
        <NumField label="선 폭" value={gap} min={0} suffix="px" onChange={setGap} />
        <NumField label="가장자리 여백" value={margin} min={0} suffix="px" onChange={setMargin} />
        <NumField label="둥글기" value={roundness} min={0} suffix="px" onChange={setRoundness} />
      </div>
      <div className="row">
        <label className="field">
          <span>배경 색상</span>
          <input type="color" value={bg} onChange={(e) => setBg(e.target.value)} />
        </label>
        <label className="field">
          <span>불투명도</span>
          <input type="range" min={0} max={255} value={bgAlpha} onChange={(e) => setBgAlpha(Number(e.target.value))} />
          <em>{Math.round((bgAlpha / 255) * 100)}%</em>
        </label>
      </div>
      <div className="row">
        <Check label="테두리 그리기" checked={drawBorder} onChange={setDrawBorder} />
        <input type="color" value={borderColor} disabled={!drawBorder} onChange={(e) => setBorderColor(e.target.value)} />
        <span className="grow" />
        <Check
          label={bgImage ? `배경 이미지: ${bgImage.split(/[\\/]/).pop()}` : "배경 이미지 사용"}
          checked={!!bgImage}
          onChange={async (v) => {
            if (!v) return setBgImage(null);
            try {
              const p = await api.pickFile(IMAGE_FILTERS);
              if (p) setBgImage(p);
            } catch (e) {
              onError(errText(e));
            }
          }}
        />
      </div>
      <div className="radio-list">
        {(
          [
            ["repeat", "반복 재생 (짧은 영상은 가장 긴 영상이 끝날 때까지 반복)"],
            ["sync", "재생 시간 동기화 (모두 가장 긴 영상 길이에 맞춰 늘림)"],
            ["sequence", "순차 재생 (한 칸씩 차례로)"],
            ["hold", "그대로 (짧은 영상은 끝 장면에서 멈춤)"],
          ] as [PlayMode, string][]
        ).map(([k, l]) => (
          <label key={k} className="check">
            <input type="radio" name="play" checked={play === k} onChange={() => setPlay(k)} />
            {l}
          </label>
        ))}
      </div>
    </Modal>
  );
}

// ---------- 파일 관리 ----------

type SortKey = "name" | "size" | "date";

export function ManageDialog({
  settings,
  onClose,
  onEdit,
}: {
  settings: Settings;
  onClose: () => void;
  onEdit: (path: string) => void;
}) {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "date", desc: true });
  const [sel, setSel] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      setFiles(await api.listFiles(settings.outputDir));
    } catch (e) {
      setError(errText(e));
    }
  };
  useEffect(() => {
    reload();
  }, []);

  const sorted = useMemo(() => {
    const f = [...files];
    f.sort((a, b) => {
      const v =
        sort.key === "name"
          ? a.name.localeCompare(b.name, undefined, { numeric: true })
          : sort.key === "size"
            ? a.size - b.size
            : a.modifiedMs - b.modifiedMs;
      return sort.desc ? -v : v;
    });
    return f;
  }, [files, sort]);

  // 고른 파일 미리보기 (GIF·WebP·APNG는 그대로 움직인다)
  useEffect(() => {
    setPreview(null);
    if (!sel || VIDEO_EXTS.includes(extOf(sel))) return;
    let url: string | null = null;
    let alive = true;
    api
      .fileBytes(sel)
      .then((buf) => {
        if (!alive) return;
        url = URL.createObjectURL(new Blob([buf]));
        setPreview(url);
      })
      .catch(() => {});
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [sel]);

  const head = (key: SortKey, label: string) => (
    <th onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : key !== "name" }))}>
      {label} {sort.key === key ? (sort.desc ? "▼" : "▲") : ""}
    </th>
  );
  const cur = files.find((f) => f.path === sel) ?? null;

  const act = async (fn: () => Promise<void>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errText(e));
    }
  };

  return (
    <Modal title="파일 관리" onClose={onClose} wide>
      <p className="hint">
        저장 폴더: {settings.outputDir} · {files.length}개
      </p>
      <div className="manage">
        <div className="manage-list">
          <table>
            <thead>
              <tr>
                {head("name", "파일명")}
                {head("size", "크기")}
                {head("date", "날짜")}
              </tr>
            </thead>
            <tbody>
              {sorted.map((f) => (
                <tr
                  key={f.path}
                  className={f.path === sel ? "sel" : ""}
                  onClick={() => setSel(f.path)}
                  onDoubleClick={() => onEdit(f.path)}
                >
                  <td>{f.name}</td>
                  <td className="num">{formatBytes(f.size)}</td>
                  <td className="num">{formatFileDate(f.modifiedMs)}</td>
                </tr>
              ))}
              {files.length === 0 && (
                <tr>
                  <td colSpan={3} className="hint">
                    저장한 파일이 없습니다.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <div className="manage-preview">
          {preview ? <img src={preview} /> : <div className="scene-empty">{cur ? "미리보기 없음" : "파일을 고르세요"}</div>}
        </div>
      </div>
      <div className="row">
        <button className="primary" disabled={!cur} onClick={() => cur && onEdit(cur.path)} title="더블클릭해도 됩니다">
          파일 편집하기
        </button>
        <button
          disabled={!cur}
          onClick={() =>
            act(async () => {
              if (!cur) return;
              const name = await promptAsk("새 파일 이름", cur.name.replace(/\.[^.]+$/, ""));
              if (!name) return;
              const np = await api.renameFile(cur.path, name);
              await reload();
              setSel(np);
            })
          }
        >
          파일 이름 변경하기
        </button>
        <button
          disabled={!cur}
          onClick={() =>
            act(async () => {
              if (
                !cur ||
                !(await confirmAsk(`${cur.name} 파일을 삭제할까요?${settings.deleteToRecycleBin ? " (휴지통으로)" : ""}`, "삭제"))
              )
                return;
              await api.deleteFile(cur.path);
              setSel(null);
              await reload();
            })
          }
        >
          파일 삭제하기
        </button>
        <span className="grow" />
        <button onClick={() => act(async () => api.revealFile(cur ? cur.path : settings.outputDir))}>현재 폴더 열기</button>
        <button onClick={() => act(reload)}>새로 고침</button>
      </div>
      {error && <p className="error">{error}</p>}
    </Modal>
  );
}

// ---------- 선택 프레임 내보내기 (Ctrl+Shift+E) ----------

export function ExportFramesDialog({
  count,
  settings,
  onClose,
  onExport,
}: {
  count: number;
  settings: Settings;
  onClose: () => void;
  onExport: (dir: string, prefix: string, format: string, openAfter: boolean) => void;
}) {
  const [dir, setDir] = useState(settings.outputDir);
  const [prefix, setPrefix] = useState("frame_%Y%m%d_%H%M%S");
  const [format, setFormat] = useState("png");
  const [openAfter, setOpenAfter] = useState(true);
  return (
    <Modal title="프레임 내보내기" onClose={onClose} okText="내보내기" okDisabled={!dir} onOk={() => onExport(dir, prefix, format, openAfter)} wide>
      <p className="hint">{count}장을 이미지 파일로 저장합니다 (이름_0001, 이름_0002 …).</p>
      <div className="row">
        <label className="field grow">
          <span>폴더</span>
          <input className="path" value={dir} onChange={(e) => setDir(e.target.value)} />
        </label>
        <button
          onClick={async () => {
            const d = await api.pickFolder(dir);
            if (d) setDir(d);
          }}
        >
          찾아보기…
        </button>
      </div>
      <label className="field">
        <span>이름</span>
        <input value={prefix} onChange={(e) => setPrefix(e.target.value)} />
      </label>
      <div className="row">
        <label className="field">
          <span>형식</span>
          <select value={format} onChange={(e) => setFormat(e.target.value)}>
            <option value="png">PNG</option>
            <option value="jpg">JPG (화질 {settings.jpgExportQuality})</option>
            <option value="webp">WebP (무손실)</option>
            <option value="bmp">BMP</option>
          </select>
        </label>
        <Check label="내보낸 뒤 폴더 열기" checked={openAfter} onChange={setOpenAfter} />
      </div>
    </Modal>
  );
}
