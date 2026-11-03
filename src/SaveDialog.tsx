// 저장 창: 왼쪽 형식 목록 + 오른쪽 그 형식의 설정(도구 패널과 같은 부품) → 저장 → 저장 뒤 할 일.

import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errText, ExportOptions, ExportResult, extOf, Format, FORMAT_EXT, formatBytes, FrameInfo, hexToRgb, Settings } from "./api";
import { IconArrowBack, IconCircleDot, IconFolderOpen, IconTrash, IconX } from "@tabler/icons-react";
import { Check, Modal } from "./dialogs";
import { ColorInput, Field, NumInput, Section, Segmented, Slider } from "./Panel";
import { chooseAsk, confirmAsk } from "./ask";

/** [형식, 이름, 한 줄 설명] */
const FORMATS: [Format, string, string][] = [
  ["webp", "WebP", "작고 선명해요 · 웹·메신저"],
  ["gif", "GIF", "어디서나 재생 · 256색"],
  ["apng", "APNG", "투명 배경을 그대로"],
  ["mp4", "MP4", "긴 영상도 아주 작게"],
  ["webm", "WebM", "웹용 영상 · 투명 지원"],
  ["avif", "AVIF", "가장 작게 · 새 브라우저"],
  ["png", "PNG 연속 이미지", "프레임마다 그림 파일"],
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
  /** 방금 저장한 파일을 지웠다 (다시 저장하지 않은 상태로) */
  onDeleted: () => void;
  onNewRecording: () => void;
}

export default function SaveDialog({ settings, frames, onClose, onSaved, onDeleted, onNewRecording }: Props) {
  const [format, setFormat] = useState<Format>("webp");
  const [quality, setQuality] = useState(settings.webpQuality);
  const [fitBySize, setFitBySize] = useState(settings.fitBySize);
  const [targetMb, setTargetMb] = useState(settings.fitTargetMb);
  const [allowQuality, setAllowQuality] = useState(settings.fitAllowQuality);
  const [allowFps, setAllowFps] = useState(settings.fitAllowFps);
  const [allowResize, setAllowResize] = useState(settings.fitAllowResize);
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
  const [done, setDone] = useState<ExportResult | null>(null);
  const [ffmpeg, setFfmpeg] = useState(true);
  // 특정 색 보호 (GIF): 이 색들은 팔레트에 그대로 남긴다
  const [protect, setProtect] = useState<string[]>([]);

  useEffect(() => {
    api.hasFfmpeg().then(setFfmpeg);
  }, []);

  // 형식이 바뀌면 확장자와 화질 기본값도 바꾼다
  useEffect(() => {
    setQuality(defaultQuality(settings, format));
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

  // 옵션이 바뀌면 예상 크기는 더 이상 맞지 않으니 지우고, 계산 중이던 결과도 버린다
  const optsKey = JSON.stringify(opts());
  const estimateRun = useRef(0);
  useEffect(() => {
    estimateRun.current++;
    setEstimate(null);
  }, [optsKey]);

  const needsFfmpeg = format === "mp4" || format === "webm" || format === "avif";
  const hasQuality = format !== "apng" && format !== "png";
  const canFit = format === "gif" || format === "webp";
  const fitting = canFit && fitBySize;
  const tooFast = format === "gif" && !limitFps && frames.some((f) => f.delayMs < 20);
  const working = progress !== null && !done && !error;

  const calc = async () => {
    const run = ++estimateRun.current;
    setEstimate("계산 중…");
    try {
      const size = await api.estimateSize(opts());
      if (run === estimateRun.current) setEstimate(formatBytes(size));
    } catch (e) {
      if (run !== estimateRun.current) return;
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
      if (format === "png") {
        const conflicts = await api.exportConflicts(o, target);
        if (conflicts.length && !(await confirmAsk("같은 이름의 파일이 있습니다. 덮어쓸까요?", "덮어쓰기"))) {
          setProgress(null);
          return;
        }
      }
      const safeTarget = Math.max(1, Math.min(4000, Math.round(targetMb) || 1));
      const result = fitting
        ? await api.exportProjectFit(o, { targetBytes: safeTarget * 1_000_000, allowQuality, allowFps, allowResize }, target)
        : await api.exportProject(o, target);
      setDone(result);
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
        fitBySize,
        fitTargetMb: safeTarget,
        fitAllowQuality: allowQuality,
        fitAllowFps: allowFps,
        fitAllowResize: allowResize,
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
        <p className="saved-path">{done.paths[0]}</p>
        {done.paths.length > 1 && <p className="hint">PNG {done.paths.length}장을 저장했습니다</p>}
        {done.size > 0 && <p className="hint">파일 크기 {formatBytes(done.size)}</p>}
        {done.fit && (
          <p>
            목표 {done.fit.targetBytes / 1_000_000}MB → 화질 {done.fit.quality},{" "}
            {done.fit.limitFps ? `${done.fit.limitFps} FPS` : "FPS 그대로"}, 크기 {done.fit.resizePercent}%로 저장했습니다
          </p>
        )}
        {done.fit && !done.fit.fitted && (
          <p className="warn">
            모든 설정을 낮춰도 목표 {done.fit.targetBytes / 1_000_000}MB보다 큽니다. 구간을 줄이거나 ‘줄여도 되는 것’을 더
            켜 보세요.
          </p>
        )}
        <div className="col">
          <button className="primary" onClick={() => api.revealFile(done.paths[0]).catch((e) => setError(errText(e)))}>
            <IconFolderOpen size={18} />
            폴더 열기
          </button>
          <button onClick={onClose}>
            <IconArrowBack size={18} />
            편집으로 돌아가기
          </button>
          <button
            onClick={async () => {
              if (!(await confirmAsk("방금 저장한 파일을 지울까요?", "지우기"))) return;
              let deleted = 0;
              try {
                for (const path of done.paths) {
                  await api.deleteFile(path);
                  deleted++;
                }
                onDeleted();
                onClose();
              } catch (e) {
                if (deleted) {
                  setDone({ ...done, paths: done.paths.slice(deleted) });
                  onDeleted();
                }
                setError(errText(e));
              }
            }}
          >
            <IconTrash size={18} />
            파일 삭제하고 돌아가기
          </button>
          <button onClick={onNewRecording}>
            <IconCircleDot size={18} />
            새로 녹화하기
          </button>
        </div>
        {error && <p className="error">{error}</p>}
      </Modal>
    );
  }

  const width = frames[0]?.width ?? 0;
  const height = frames[0]?.height ?? 0;
  const outW = Math.max(1, Math.floor((width * (resize || 100)) / 100));
  const outH = Math.max(1, Math.floor((height * (resize || 100)) / 100));

  return (
    <Modal title="저장" onClose={() => !working && onClose()} className="save-modal">
      <div className="save-grid">
        {/* 왼쪽: 형식 목록 (한 줄 설명으로 무엇을 고를지 돕는다) */}
        <div className="save-formats" role="radiogroup" aria-label="저장 형식">
          {FORMATS.map(([f, label, desc]) => {
            const noFfmpeg = (f === "mp4" || f === "webm" || f === "avif") && !ffmpeg;
            return (
              <button
                key={f}
                role="radio"
                aria-checked={f === format}
                className={f === format ? "on" : ""}
                disabled={working}
                onClick={() => setFormat(f)}
              >
                <span>{label}</span>
                <small className={noFfmpeg ? "need" : ""}>{noFfmpeg ? "ffmpeg가 필요해요" : desc}</small>
              </button>
            );
          })}
        </div>

        {/* 오른쪽: 고른 형식의 설정 (길면 이 부분만 스크롤) + 늘 보이는 결과 요약 */}
        <div className="save-right">
          <div className="save-settings">
            {needsFfmpeg && !ffmpeg && (
              <p className="error save-note">ffmpeg.exe를 찾지 못했습니다. ffmpeg를 설치하거나 OpenCam 폴더에 넣어 주세요.</p>
            )}
            {hasQuality && (
              <Section title={canFit ? "화질·용량" : "화질"}>
                {/* 용량에 맞추기 (GIF·WebP): 목표 크기를 정하면 화질·FPS·크기를 알아서 낮춘다 */}
                {canFit && (
                  <Field label="기준">
                    <Segmented
                      label="기준"
                      value={fitBySize ? "size" : "quality"}
                      onChange={(v) => setFitBySize(v === "size")}
                      options={[
                        ["quality", "화질 직접 정하기"],
                        ["size", "용량에 맞추기"],
                      ]}
                    />
                  </Field>
                )}
                {fitting ? (
                  <>
                    <Field label="목표">
                      <NumInput value={targetMb} min={1} max={4000} unit="MB" label="목표 크기" disabled={working} onChange={setTargetMb} />
                      <Segmented
                        label="자주 쓰는 목표 크기"
                        value={String(targetMb)}
                        onChange={(v) => setTargetMb(Number(v))}
                        options={[
                          ["5", "5"],
                          ["10", "10"],
                          ["20", "20"],
                          ["50", "50"],
                        ]}
                      />
                    </Field>
                    <Field label="줄여도 됨">
                      <Check label="화질" checked={allowQuality} onChange={setAllowQuality} disabled={working} />
                      <Check label="FPS" checked={allowFps} onChange={setAllowFps} disabled={working} />
                      <Check label="이미지 크기" checked={allowResize} onChange={setAllowResize} disabled={working} />
                    </Field>
                    <p className="hint">화질 → FPS → 이미지 크기 순으로 낮춰 목표 안에 맞춥니다</p>
                  </>
                ) : (
                  <Field label="화질">
                    <Slider label="화질" value={quality} min={1} max={100} disabled={working} onChange={setQuality} />
                  </Field>
                )}
              </Section>
            )}

            <Section title="파일 크기 줄이기">
              <Field label="FPS 제한">
                <Check label="" checked={limitFps} onChange={setLimitFps} disabled={working} />
                <NumInput value={limitFpsValue} min={1} max={100} unit="FPS" label="FPS 제한" disabled={!limitFps || working} onChange={setLimitFpsValue} />
              </Field>
              <Field label="비슷한 장면">
                <Check label="" checked={skipSimilar} onChange={setSkipSimilar} disabled={working} />
                <NumInput value={skipPct} min={0} max={50} unit="%" label="비슷한 이미지 건너뛰기" disabled={!skipSimilar || working} onChange={setSkipPct} />
                <span className="hint">이하로 다르면 건너뛰기</span>
              </Field>
              <Field label="시간 제한" sub>
                <Check label="" checked={skipTime} onChange={setSkipTime} disabled={!skipSimilar || working} />
                <NumInput
                  value={skipTimeMs}
                  min={10}
                  unit="ms"
                  label="건너뛰기 시간 제한"
                  wide
                  disabled={!skipSimilar || !skipTime || working}
                  onChange={setSkipTimeMs}
                />
              </Field>
              <Field label="이미지 크기">
                <NumInput value={resize} min={1} max={400} unit="%" label="이미지 크기" disabled={working} onChange={setResize} />
                <span className="hint">
                  → {outW} × {outH}
                </span>
              </Field>
            </Section>

            <Section title="추가 옵션">
              <Field label="진행 막대">
                <Check label="재생 진행 막대 넣기" checked={progressBar} onChange={setProgressBar} disabled={working} />
              </Field>
              {format === "gif" && (
                <Field label="색 보호">
                  <div className="save-colors">
                    {protect.map((c, i) => (
                      <span key={i} className="save-color">
                        <ColorInput
                          label={`보호할 색 ${i + 1}`}
                          value={c}
                          disabled={working}
                          onChange={(v) => setProtect((p) => p.map((x, k) => (k === i ? v : x)))}
                        />
                        <button
                          className="icon ghost"
                          disabled={working}
                          onClick={() => setProtect((p) => p.filter((_, k) => k !== i))}
                          title="빼기"
                        >
                          <IconX size={14} />
                        </button>
                      </span>
                    ))}
                    <button disabled={working || protect.length >= 16} onClick={() => setProtect((p) => [...p, "#ff0000"])}>
                      + 보호할 색
                    </button>
                    {protect.length > 0 && <p className="hint">이 색들은 GIF에서 그대로 남습니다</p>}
                  </div>
                </Field>
              )}
            </Section>
          </div>

          {/* 결과 요약: 크기·장수와 예상 크기 */}
          <div className="save-sum">
            <span>
              {outW} × {outH} · {frames.length}장{fitting && ` · 목표 ${targetMb}MB 안으로`}
            </span>
            {!fitting && (
              <>
                <span className="grow" />
                {estimate && <b>{estimate}</b>}
                <button onClick={calc} disabled={working || (needsFfmpeg && !ffmpeg)}>
                  예상 크기 계산
                </button>
              </>
            )}
          </div>
          {tooFast && <p className="warn save-note">20ms보다 짧은 프레임이 있어 브라우저에서 느리게 재생될 수 있습니다.</p>}
        </div>
      </div>

      <div className="save-foot">
        {working && progress && (
          <div className="progress">
            <div className="bar" style={{ width: `${Math.round(progress.p * 100)}%` }} />
            <span>
              {progress.msg} {Math.round(progress.p * 100)}%
            </span>
          </div>
        )}
        {error && <p className="error">{error}</p>}
        <div className="save-foot-row">
          <input className="path" aria-label="저장할 파일" value={path} onChange={(e) => setPath(e.target.value)} disabled={working} />
          <button
            disabled={working}
            onClick={async () => {
              const p = await api.pickSavePath(path, FORMAT_EXT[format]);
              if (p) setPath(p);
            }}
          >
            찾아보기…
          </button>
          <button onClick={onClose} disabled={working}>
            취소
          </button>
          <button className="primary" onClick={save} disabled={working || !path || (needsFfmpeg && !ffmpeg)}>
            {working ? "저장 중…" : fitting ? "용량에 맞춰 저장" : "저장"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
