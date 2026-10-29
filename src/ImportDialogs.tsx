// 열기 대화상자: 동영상 구간 고르기, 이미지 여러 장 옵션.

import { useEffect, useState } from "react";
import { api, errText, Fit, VideoInfo } from "./api";
import { Modal, NumField } from "./dialogs";

/** 동영상 한 장면을 미리 보여준다 (구간 시작·끝 확인용) */
function Scene({ path, atMs, label }: { path: string; atMs: number; label: string }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    let made: string | null = null;
    const t = window.setTimeout(async () => {
      try {
        const buf = await api.videoFrame(path, atMs);
        if (!alive) return;
        made = URL.createObjectURL(new Blob([buf], { type: "image/png" }));
        setUrl(made);
      } catch {
        if (alive) setUrl(null);
      }
    }, 250);
    return () => {
      alive = false;
      window.clearTimeout(t);
      if (made) window.setTimeout(() => URL.revokeObjectURL(made!), 2000);
    };
  }, [path, atMs]);
  return (
    <figure className="scene">
      {url ? <img src={url} /> : <div className="scene-empty">…</div>}
      <figcaption>{label}</figcaption>
    </figure>
  );
}

const sec = (ms: number) => Math.round(ms / 100) / 10;

export function VideoImportDialog({
  path,
  info,
  defaultFps,
  onClose,
  onImport,
}: {
  path: string;
  info: VideoInfo;
  defaultFps: number;
  onClose: () => void;
  onImport: (startMs: number, endMs: number, fps: number, w: number, h: number) => void;
}) {
  const dur = info.durationMs;
  const [start, setStart] = useState(0);
  // 긴 동영상을 통째로 불러오면 메모리가 모자라기 쉬워 처음 15초만 골라 둔다
  const [end, setEnd] = useState(Math.min(dur, 15000));
  const [fps, setFps] = useState(Math.min(defaultFps, Math.round(info.fps)) || 15);
  const [pct, setPct] = useState(info.width > 1280 ? Math.floor((1280 / info.width) * 100) : 100);
  const w = Math.max(1, Math.round((info.width * pct) / 100));
  const h = Math.max(1, Math.round((info.height * pct) / 100));
  const valid = end > start && fps >= 1 && fps <= 60 && pct >= 1 && pct <= 100;
  const count = Math.ceil(((end - start) / 1000) * fps);
  const name = path.split(/[\\/]/).pop();

  return (
    <Modal
      title="동영상 열기"
      onClose={onClose}
      onOk={() => onImport(start, end, fps, w, h)}
      okDisabled={!valid}
      okText="불러오기"
      wide
    >
      <p className="hint">
        {name} · {info.width} × {info.height} · {(dur / 1000).toFixed(1)}초 · {info.fps.toFixed(1)} FPS
      </p>
      <div className="scenes">
        <Scene path={path} atMs={start} label={`시작 ${sec(start)}초`} />
        <Scene path={path} atMs={Math.max(start, end - 50)} label={`끝 ${sec(end)}초`} />
      </div>
      <label className="field">
        <span>시작</span>
        <input
          type="range"
          min={0}
          max={dur}
          step={100}
          value={start}
          onChange={(e) => {
            const v = Number(e.target.value);
            setStart(Math.min(v, end - 100));
          }}
        />
        <NumField label="" value={sec(start)} min={0} suffix="초" onChange={(v) => v >= 0 && setStart(Math.min(v * 1000, end - 100))} />
      </label>
      <label className="field">
        <span>끝</span>
        <input
          type="range"
          min={0}
          max={dur}
          step={100}
          value={end}
          onChange={(e) => {
            const v = Number(e.target.value);
            setEnd(Math.max(v, start + 100));
          }}
        />
        <NumField label="" value={sec(end)} min={0} suffix="초" onChange={(v) => v > 0 && setEnd(Math.min(dur, Math.max(v * 1000, start + 100)))} />
      </label>
      <div className="row">
        <NumField label="FPS" value={fps} min={1} max={60} onChange={setFps} />
        <NumField label="크기" value={pct} min={1} max={100} suffix="%" onChange={setPct} />
        <span className="hint">
          → {w} × {h}
        </span>
      </div>
      <p className="hint">
        {sec(end - start)}초 구간 · 약 {Number.isFinite(count) ? count : 0}장
        {count > 1500 && " — 장수가 많아 메모리 한도에 걸릴 수 있습니다"}
      </p>
    </Modal>
  );
}

export function ImagesImportDialog({
  paths,
  onClose,
  onImport,
}: {
  paths: string[];
  onClose: () => void;
  onImport: (stillMs: number, fit: Fit) => void;
}) {
  const [ms, setMs] = useState(100);
  const [fit, setFit] = useState<Fit>("contain");
  const [error, setError] = useState<string | null>(null);
  return (
    <Modal
      title="이미지 열기"
      onClose={onClose}
      onOk={() => {
        if (!(ms >= 10)) return setError("시간은 10ms 이상이어야 합니다");
        onImport(Math.round(ms), fit);
      }}
      okText="불러오기"
    >
      <p className="hint">
        {paths.length}개 파일을 이름 순서대로 이어 붙입니다. 크기는 첫 번째 그림에 맞춥니다.
      </p>
      <NumField label="한 장의 시간" value={ms} min={10} suffix="ms" onChange={setMs} />
      <p className="hint">GIF·WebP·APNG처럼 움직이는 파일은 원래 시간을 그대로 씁니다.</p>
      <div className="radio-list">
        <label className="check">
          <input type="radio" name="fit" checked={fit === "contain"} onChange={() => setFit("contain")} />
          비율 유지 (남는 곳은 투명)
        </label>
        <label className="check">
          <input type="radio" name="fit" checked={fit === "stretch"} onChange={() => setFit("stretch")} />
          꽉 채우기 (비율 무시)
        </label>
      </div>
      {error && <p className="error">{errText(error)}</p>}
    </Modal>
  );
}
