// 저장 창: 형식·화질·파일 크기 줄이기 → 저장 → 저장 뒤 할 일.

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errText, ExportOptions, extOf, Format, FORMAT_EXT, formatBytes, FrameInfo, hexToRgb, Settings } from "./api";
import { Check, Modal, NumField } from "./dialogs";
import { chooseAsk, confirmAsk } from "./ask";

const FORMATS: [Format, string][] = [
  ["gif", "GIF"],
  ["webp", "WebP"],
  ["apng", "APNG"],
  ["mp4", "MP4"],
  ["webm", "WebM"],
  ["avif", "AVIF"],
  ["png", "PNG 연속 이미지"],
];

function defaultQuality(s: Settings, f: Format): number {
  return {
    gif: s.gifQuality,
    webp: s.webpQuality,
    apng: 100,
    mp4: s.mp4Quality,
    webm: s.webmQuality,
    png: 100,
    avif: s.avifQuality,
  }[f];
}

interface Props {
  settings: Settings;
  frames: FrameInfo[];
  onClose: () => void;
  onSaved: () => void;
  onNewRecording: () => void;
}

export default function SaveDialog({ settings, frames, onClose, onSaved, onNewRecording }: Props) {
  const [format, setFormat] = useState<Format>("gif");
  const [quality, setQuality] = useState(settings.gifQuality);
  const [path, setPath] = useState("");
  const [limitFps, setLimitFps] = useState(settings.limitFps);
  const [limitFpsValue, setLimitFpsValue] = useState(settings.limitFpsValue);
  const [skipSimilar, setSkipSimilar] = useState(settings.skipSimilar);
  const [skipPct, setSkipPct] = useState(settings.skipSimilarPercent);
  const [skipTime, setSkipTime] = useState(settings.skipSimilarTimeLimit);
  const [skipTimeMs, setSkipTimeMs] = useState(settings.skipSimilarTimeLimitMs);
  const [resize, setResize] = useState(settings.resizePercent);
  const [progressBar, setProgressBar] = useState(settings.embedProgress);
  const [estimate, setEstimate] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ p: number; msg: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ path: string; size: number } | null>(null);
  const [ffmpeg, setFfmpeg] = useState(true);
  // 특정 색 보호 (GIF): 이 색들은 팔레트에 그대로 남긴다
  const [protect, setProtect] = useState<string[]>([]);

  useEffect(() => {
    api.hasFfmpeg().then(setFfmpeg);
  }, []);

  // 형식이 바뀌면 확장자와 화질 기본값도 바꾼다
  useEffect(() => {
    setQuality(defaultQuality(settings, format));
    setEstimate(null);
    const ext = FORMAT_EXT[format];
    if (!path) api.defaultSavePath(ext).then(setPath);
    else setPath((p) => p.replace(/\.[^.\\/]+$/, "") + "." + ext);
  }, [format]);

  const opts = (): ExportOptions => ({
    format,
    quality: Math.max(1, Math.min(100, Math.round(quality))),
    limitFps: limitFps ? Math.max(1, Math.round(limitFpsValue)) : null,
    skipSimilar: skipSimilar ? Math.max(0, Math.min(50, Math.round(skipPct))) : null,
    skipSimilarTimeLimitMs: skipSimilar && skipTime ? Math.max(10, Math.round(skipTimeMs)) : null,
    resizePercent: Math.max(1, Math.min(400, Math.round(resize) || 100)),
    embedProgress: progressBar,
    protectColors: format === "gif" ? protect.map(hexToRgb) : [],
  });

  const needsFfmpeg = format === "mp4" || format === "webm" || format === "avif";
  const hasQuality = format !== "apng" && format !== "png";
  const tooFast = format === "gif" && !limitFps && frames.some((f) => f.delayMs < 20);
  const working = progress !== null && !done && !error;

  const calc = async () => {
    setEstimate("계산 중…");
    try {
      setEstimate(formatBytes(await api.estimateSize(opts())));
    } catch (e) {
      setEstimate(null);
      setError(errText(e));
    }
  };

  const save = async () => {
    if (!path) return;
    // 파일 이름의 확장자가 형식과 다르면(GIF인데 .png로 적은 경우 등) 형식에 맞게 바꾼다
    const ext = FORMAT_EXT[format];
    const target = extOf(path) === ext ? path : path.replace(/\.[^.\\/]+$/, "") + "." + ext;
    if (target !== path) setPath(target);
    // GIF는 프레임 시간을 20ms보다 짧게 쓸 수 없다. 이번 저장에만 50 FPS로 다시 뽑을지 고른다 (편집 중인 프레임은 그대로).
    let fit50 = false;
    if (tooFast && !settings.doNotAskGif50fps) {
      const pick = await chooseAsk(
        "20ms보다 짧은 프레임이 있습니다(50 FPS 초과). GIF는 프레임 시간을 20ms보다 짧게 저장할 수 없습니다.\n\n" +
          "• 50 FPS로 맞춰 저장: 0.02초마다 그때의 화면을 골라 원래 속도로 저장합니다(60 FPS면 6장 중 1장이 빠짐). 편집 중인 프레임은 그대로 남습니다.\n" +
          "• 그대로 저장: 프레임은 모두 남지만 짧은 프레임이 20ms로 늘어나 조금 느리게 재생됩니다.",
        ["50 FPS로 맞춰 저장", "그대로 저장"],
      );
      if (pick === null) return;
      fit50 = pick === 0;
    }
    setError(null);
    setProgress({ p: 0, msg: "준비 중" });
    const un = await listen<{ progress: number; message: string }>("export-progress", (e) =>
      setProgress({ p: e.payload.progress, msg: e.payload.message }),
    );
    try {
      const o = { ...opts(), gifFit50fps: fit50 };
      const size = await api.exportProject(o, target);
      setDone({ path: target, size });
      onSaved();
      // 이번에 고른 옵션을 다음 저장의 기본값으로
      const q: Partial<Settings> =
        format === "gif"
          ? { gifQuality: o.quality }
          : format === "webp"
            ? { webpQuality: o.quality }
            : format === "mp4"
              ? { mp4Quality: o.quality }
              : format === "webm"
                ? { webmQuality: o.quality }
                : format === "avif"
                  ? { avifQuality: o.quality }
                  : {};
      await api.setSettings({
        ...settings,
        ...q,
        limitFps,
        limitFpsValue: o.limitFps ?? settings.limitFpsValue,
        skipSimilar,
        skipSimilarPercent: o.skipSimilar ?? settings.skipSimilarPercent,
        skipSimilarTimeLimit: skipTime,
        skipSimilarTimeLimitMs: o.skipSimilarTimeLimitMs ?? settings.skipSimilarTimeLimitMs,
        resizePercent: o.resizePercent,
        embedProgress: progressBar,
      });
    } catch (e) {
      setError(errText(e));
      setProgress(null);
    } finally {
      un();
    }
  };

  if (done) {
    return (
      <Modal title="저장했습니다" onClose={onClose}>
        <p className="saved-path">{done.path}</p>
        {done.size > 0 && <p className="hint">파일 크기 {formatBytes(done.size)}</p>}
        <div className="col">
          <button className="primary" onClick={() => api.revealFile(done.path).catch((e) => setError(errText(e)))}>
            📂 폴더 열기
          </button>
          <button onClick={onClose}>편집으로 돌아가기</button>
          <button
            onClick={async () => {
              if (!(await confirmAsk("방금 저장한 파일을 지울까요?", "지우기"))) return;
              try {
                await api.deleteFile(done.path);
                onClose();
              } catch (e) {
                setError(errText(e));
              }
            }}
          >
            🗑 파일 삭제하고 돌아가기
          </button>
          <button onClick={onNewRecording}>● 새로 녹화하기</button>
        </div>
        {error && <p className="error">{error}</p>}
      </Modal>
    );
  }

  return (
    <Modal title="저장" onClose={() => !working && onClose()} wide>
      <div className="row">
        <label className="field grow">
          <span>파일</span>
          <input className="path" value={path} onChange={(e) => setPath(e.target.value)} disabled={working} />
        </label>
        <button
          disabled={working}
          onClick={async () => {
            const p = await api.pickSavePath(path, FORMAT_EXT[format]);
            if (p) setPath(p);
          }}
        >
          찾아보기…
        </button>
      </div>

      <div className="formats">
        {FORMATS.map(([f, label]) => (
          <button key={f} className={f === format ? "on" : ""} disabled={working} onClick={() => setFormat(f)}>
            {label}
          </button>
        ))}
      </div>
      {needsFfmpeg && !ffmpeg && (
        <p className="error">ffmpeg.exe를 찾지 못했습니다. ffmpeg를 설치하거나 OpenCam 폴더에 넣어 주세요.</p>
      )}

      {hasQuality && (
        <label className="field">
          <span>화질</span>
          <input
            type="range"
            min={1}
            max={100}
            value={quality}
            disabled={working}
            onChange={(e) => {
              setQuality(Number(e.target.value));
              setEstimate(null);
            }}
          />
          <em>{quality}</em>
        </label>
      )}

      <h3>파일 크기 줄이기</h3>
      <div className="group">
        <div className="row">
          <Check label="FPS 제한" checked={limitFps} onChange={setLimitFps} disabled={working} />
          <NumField label="" value={limitFpsValue} min={1} max={100} suffix="FPS" disabled={!limitFps || working} onChange={setLimitFpsValue} />
        </div>
        <div className="row">
          <Check label="비슷한 이미지 건너뛰기" checked={skipSimilar} onChange={setSkipSimilar} disabled={working} />
          <NumField label="" value={skipPct} min={0} max={50} suffix="%" disabled={!skipSimilar || working} onChange={setSkipPct} />
          <Check label="시간 제한" checked={skipTime} onChange={setSkipTime} disabled={!skipSimilar || working} />
          <NumField label="" value={skipTimeMs} min={10} suffix="ms" disabled={!skipSimilar || !skipTime || working} onChange={setSkipTimeMs} />
        </div>
        <div className="row">
          <NumField label="이미지 크기" value={resize} min={1} max={400} suffix="%" disabled={working} onChange={setResize} />
          <span className="hint">
            → {Math.max(1, Math.floor(((frames[0]?.width ?? 0) * (resize || 100)) / 100))} ×{" "}
            {Math.max(1, Math.floor(((frames[0]?.height ?? 0) * (resize || 100)) / 100))}
          </span>
        </div>
      </div>
      <Check label="재생 진행 막대 넣기" checked={progressBar} onChange={setProgressBar} disabled={working} />
      {format === "gif" && (
        <div className="row">
          <span className="hint">색상 보호하기</span>
          {protect.map((c, i) => (
            <span key={i} className="row protect-color">
              <input
                type="color"
                value={c}
                disabled={working}
                onChange={(e) => setProtect((p) => p.map((x, k) => (k === i ? e.target.value : x)))}
              />
              <button disabled={working} onClick={() => setProtect((p) => p.filter((_, k) => k !== i))} title="빼기">
                ✕
              </button>
            </span>
          ))}
          <button disabled={working || protect.length >= 16} onClick={() => setProtect((p) => [...p, "#ff0000"])}>
            + 보호할 색
          </button>
          {protect.length > 0 && <span className="hint">(이 색들은 GIF에서 그대로 남습니다)</span>}
        </div>
      )}
      {tooFast && <p className="warn">20ms보다 짧은 프레임이 있어 브라우저에서 느리게 재생될 수 있습니다.</p>}

      {working && progress && (
        <div className="progress">
          <div className="bar" style={{ width: `${Math.round(progress.p * 100)}%` }} />
          <span>
            {progress.msg} {Math.round(progress.p * 100)}%
          </span>
        </div>
      )}
      {error && <p className="error">{error}</p>}

      <div className="modal-foot">
        <button onClick={calc} disabled={working || (needsFfmpeg && !ffmpeg)}>
          예상 크기 계산
        </button>
        {estimate && <span className="hint">{estimate}</span>}
        <span className="grow" />
        <button onClick={onClose} disabled={working}>
          취소
        </button>
        <button className="primary" onClick={save} disabled={working || !path || (needsFfmpeg && !ffmpeg)}>
          {working ? "저장 중…" : "저장"}
        </button>
      </div>
    </Modal>
  );
}
