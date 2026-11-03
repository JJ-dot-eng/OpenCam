// 저장 창: 왼쪽 형식 목록 + 오른쪽 그 형식의 설정(도구 패널과 같은 부품) → 저장 → 저장 뒤 할 일.

import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  bgrToHex,
  errText,
  ExportOptions,
  ExportResult,
  extOf,
  Format,
  FORMAT_EXT,
  formatBytes,
  FrameInfo,
  hexToBgr,
  hexToRgb,
  Settings,
} from "./api";
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

/** 형식마다 따로 정하는 저장 옵션 (예전에는 설정 창의 형식 탭에 있었다) */
const FORMAT_KEYS = [
  "gifDither",
  "gifRemoveSimilarPixels",
  "gifFiniteLoop",
  "gifLoopCount",
  "webpMethod",
  "webpLossless",
  "webpIphoneCompat",
  "webpFiniteLoop",
  "webpLoopCount",
  "apng256Colors",
  "apngFiniteLoop",
  "apngLoopCount",
  "mp4ReplaceTransparent",
  "transparentReplaceColor",
] as const;
type FormatOpts = Pick<Settings, (typeof FORMAT_KEYS)[number]>;
type LoopKeys = ["gifFiniteLoop", "gifLoopCount"] | ["webpFiniteLoop", "webpLoopCount"] | ["apngFiniteLoop", "apngLoopCount"];

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
  const [fmt, setFmt] = useState<FormatOpts>(() => Object.fromEntries(FORMAT_KEYS.map((k) => [k, settings[k]])) as FormatOpts);
  const setF = <K extends keyof FormatOpts>(k: K, v: FormatOpts[K]) => setFmt((c) => ({ ...c, [k]: v }));
  // 저장·예상 크기 계산은 Rust가 가진 설정으로 하므로 형식별 옵션을 먼저 넣어 둔다.
  // 단축키 등록이 실패해도 설정 자체는 저장되므로 그 오류는 넘긴다.
  const applyFmt = () => api.setSettings({ ...settings, ...fmt }).catch(() => {});

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
  const optsKey = JSON.stringify([opts(), fmt]);
  const estimateRun = useRef(0);
  useEffect(() => {
    estimateRun.current++;
    setEstimate(null);
  }, [optsKey]);

  const needsFfmpeg = format === "mp4" || format === "webm" || format === "avif";
  const hasQuality = format !== "apng" && format !== "png";
  const canFit = format === "gif" || format === "webp";
  const fitting = canFit && fitBySize;
  // 용량에 맞추기에서 "변경 가능"으로 고른 항목은 아래 설정을 무시하고 알아서 정한다 (가장 큰 값에서 시작)
  const autoFps = fitting && allowFps;
  const autoResize = fitting && allowResize;
  const tooFast = format === "gif" && (autoFps || !limitFps) && frames.some((f) => f.delayMs < 20);
  const working = progress !== null && !done && !error;

  const calc = async () => {
    const run = ++estimateRun.current;
    setEstimate("계산 중…");
    try {
      await applyFmt();
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
      await applyFmt();
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
        ...fmt,
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
            모든 설정을 낮춰도 목표 {done.fit.targetBytes / 1_000_000}MB보다 큽니다. 구간을 줄이거나 ‘변경 가능’을 더
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

  // 반복: 무한 / 횟수 지정 (GIF·WebP·APNG)
  const loopField = ([finite, count]: LoopKeys) => (
    <Field label="반복">
      <Segmented
        label="반복"
        value={fmt[finite] ? "n" : "inf"}
        onChange={(v) => setF(finite, v === "n")}
        options={[
          ["inf", "무한 반복"],
          ["n", "횟수 지정"],
        ]}
      />
      {fmt[finite] && (
        <NumInput
          value={fmt[count]}
          min={1}
          max={65535}
          unit="번"
          label="재생 횟수"
          disabled={working}
          onChange={(v) => Number.isFinite(v) && setF(count, Math.max(1, Math.min(65535, Math.round(v))))}
        />
      )}
    </Field>
  );
  const loopText = (finite: boolean, count: number) => (finite ? `${count}번 재생` : "무한 반복");
  // 접혀 있을 때 보여 줄 지금 설정 요약
  const extraSummary =
    [
      format === "gif" && loopText(fmt.gifFiniteLoop, fmt.gifLoopCount),
      format === "gif" && fmt.gifDither && "디더링",
      format === "gif" && protect.length > 0 && `보호 색 ${protect.length}개`,
      format === "webp" && loopText(fmt.webpFiniteLoop, fmt.webpLoopCount),
      format === "webp" && (fmt.webpLossless ? "무손실" : `압축 ${fmt.webpMethod}`),
      format === "apng" && (fmt.apng256Colors ? "256색" : "24비트"),
      format === "apng" && loopText(fmt.apngFiniteLoop, fmt.apngLoopCount),
      format === "mp4" && fmt.mp4ReplaceTransparent && "투명 부분 색 바꿈",
      progressBar && "진행 막대",
    ]
      .filter(Boolean)
      .join(" · ") || "기본값";

  const width = frames[0]?.width ?? 0;
  const height = frames[0]?.height ?? 0;
  // 크기를 알아서 정할 때는 원래 크기가 가장 큰 값
  const outPct = autoResize ? 100 : resize || 100;
  const outW = Math.max(1, Math.floor((width * outPct) / 100));
  const outH = Math.max(1, Math.floor((height * outPct) / 100));
  /** 용량에 맞추기가 정하는 칸에 붙이는 말 */
  const autoNote = <span className="hint">용량에 맞추기가 정함</span>;

  return (
    <Modal title="저장" onClose={() => !working && onClose()} className="pane-modal">
      <div className="pane-grid">
        {/* 왼쪽: 형식 목록 (한 줄 설명으로 무엇을 고를지 돕는다) */}
        <div className="pane-nav" role="radiogroup" aria-label="저장 형식">
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
        <div className="pane-right">
          <div className="pane-body">
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
                    <Field label="변경 가능">
                      <Check label="화질" checked={allowQuality} onChange={setAllowQuality} disabled={working} />
                      <Check label="FPS" checked={allowFps} onChange={setAllowFps} disabled={working} />
                      <Check label="이미지 크기" checked={allowResize} onChange={setAllowResize} disabled={working} />
                    </Field>
                    <p className="hint">
                      고른 항목은 가장 높은 값(화질 100, 원래 FPS, 원래 크기)에서 시작해 화질 → FPS → 이미지 크기 순으로 낮추며 목표에 가장
                      가깝게 맞춥니다. 고르지 않은 항목은 직접 정한 값 그대로 씁니다.
                    </p>
                    {!allowQuality && (
                      <Field label="화질">
                        <Slider label="화질" value={quality} min={1} max={100} disabled={working} onChange={setQuality} />
                      </Field>
                    )}
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
                <Check label="" checked={limitFps} onChange={setLimitFps} disabled={working || autoFps} />
                <NumInput
                  value={limitFpsValue}
                  min={1}
                  max={100}
                  unit="FPS"
                  label="FPS 제한"
                  disabled={!limitFps || working || autoFps}
                  onChange={setLimitFpsValue}
                />
                {autoFps && autoNote}
              </Field>
              <Field label="비슷한 장면">
                <Check label="" checked={skipSimilar} onChange={setSkipSimilar} disabled={working || autoFps} />
                <NumInput
                  value={skipPct}
                  min={0}
                  max={50}
                  unit="%"
                  label="비슷한 이미지 건너뛰기"
                  disabled={!skipSimilar || working || autoFps}
                  onChange={setSkipPct}
                />
                {autoFps ? autoNote : <span className="hint">이하로 다르면 건너뛰기</span>}
              </Field>
              <Field label="시간 제한" sub>
                <Check label="" checked={skipTime} onChange={setSkipTime} disabled={!skipSimilar || working || autoFps} />
                <NumInput
                  value={skipTimeMs}
                  min={10}
                  unit="ms"
                  label="건너뛰기 시간 제한"
                  wide
                  disabled={!skipSimilar || !skipTime || working || autoFps}
                  onChange={setSkipTimeMs}
                />
              </Field>
              <Field label="이미지 크기">
                <NumInput value={resize} min={1} max={400} unit="%" label="이미지 크기" disabled={working || autoResize} onChange={setResize} />
                {autoResize ? (
                  autoNote
                ) : (
                  <span className="hint">
                    → {outW} × {outH}
                  </span>
                )}
              </Field>
            </Section>

            <Section title="추가 옵션" collapsible defaultOpen={false} summary={extraSummary}>
              {format === "gif" && (
                <>
                  {loopField(["gifFiniteLoop", "gifLoopCount"])}
                  <Field label="디더링">
                    <Check label="색 띠를 점으로 섞기 (조금 커짐)" checked={fmt.gifDither} onChange={(v) => setF("gifDither", v)} disabled={working} />
                  </Field>
                  <Field label="비슷한 픽셀">
                    <Check label="합치기 (작아짐)" checked={fmt.gifRemoveSimilarPixels} onChange={(v) => setF("gifRemoveSimilarPixels", v)} disabled={working} />
                  </Field>
                </>
              )}
              {format === "webp" && (
                <>
                  {loopField(["webpFiniteLoop", "webpLoopCount"])}
                  <Field label="압축 방식">
                    <NumInput
                      value={fmt.webpMethod}
                      min={0}
                      max={6}
                      label="압축 방식"
                      narrow
                      disabled={working}
                      onChange={(v) => Number.isFinite(v) && setF("webpMethod", Math.max(0, Math.min(6, Math.round(v))))}
                    />
                    <span className="hint">0 빠름 ~ 6 작게</span>
                  </Field>
                  <Field label="무손실">
                    <Check label="색을 전혀 잃지 않기 (커짐)" checked={fmt.webpLossless} onChange={(v) => setF("webpLossless", v)} disabled={working} />
                  </Field>
                  <Field label="아이폰">
                    <Check label="아이폰 호환 (프레임을 통째로)" checked={fmt.webpIphoneCompat} onChange={(v) => setF("webpIphoneCompat", v)} disabled={working} />
                  </Field>
                </>
              )}
              {format === "apng" && (
                <>
                  <Field label="색">
                    <Segmented
                      label="APNG 색"
                      value={fmt.apng256Colors ? "256" : "24"}
                      onChange={(v) => setF("apng256Colors", v === "256")}
                      options={[
                        ["256", "256색 (작게)"],
                        ["24", "24비트 (색 그대로)"],
                      ]}
                    />
                  </Field>
                  {loopField(["apngFiniteLoop", "apngLoopCount"])}
                </>
              )}
              {format === "mp4" && (
                <Field label="투명 부분">
                  <Check label="이 색으로" checked={fmt.mp4ReplaceTransparent} onChange={(v) => setF("mp4ReplaceTransparent", v)} disabled={working} />
                  <ColorInput
                    label="투명 부분 색"
                    value={bgrToHex(fmt.transparentReplaceColor)}
                    disabled={!fmt.mp4ReplaceTransparent || working}
                    onChange={(v) => setF("transparentReplaceColor", hexToBgr(v))}
                  />
                </Field>
              )}
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
              {autoResize && "최대 "}
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

      <div className="pane-foot">
        {working && progress && (
          <div className="progress">
            <div className="bar" style={{ width: `${Math.round(progress.p * 100)}%` }} />
            <span>
              {progress.msg} {Math.round(progress.p * 100)}%
            </span>
          </div>
        )}
        {error && <p className="error">{error}</p>}
        <div className="pane-foot-row">
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
