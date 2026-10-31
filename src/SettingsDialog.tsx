// 설정 창.

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, bgrToHex, errText, hexToBgr, Settings } from "./api";
import { Check, Modal, NumField } from "./dialogs";
import { alertAsk } from "./ask";
import { checkForUpdate, REPO_URL } from "./updater";

const TABS = ["일반", "녹화", "편집", "GIF", "WebP", "APNG", "동영상", "단축키", "정보"] as const;
type Tab = (typeof TABS)[number];

interface Props {
  settings: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
}

export default function SettingsDialog({ settings, onClose, onSaved }: Props) {
  const [s, setS] = useState<Settings>(settings);
  const [tab, setTab] = useState<Tab>("일반");
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((cur) => ({ ...cur, [k]: v }));
  const num = (k: keyof Settings, lo: number, hi: number) => (v: number) =>
    Number.isFinite(v) && set(k, Math.max(lo, Math.min(hi, Math.round(v))) as never);
  const color = (label: string, k: keyof Settings) => (
    <label className="field">
      <span>{label}</span>
      <input type="color" value={bgrToHex(s[k] as number)} onChange={(e) => set(k, hexToBgr(e.target.value) as never)} />
    </label>
  );
  const loop = (finite: keyof Settings, count: keyof Settings) => (
    <div className="row">
      <Check label="반복 횟수 지정 (끄면 무한 반복)" checked={s[finite] as boolean} onChange={(v) => set(finite, v as never)} />
      <NumField label="" value={s[count] as number} min={1} max={65535} suffix="번 재생" disabled={!s[finite]} onChange={num(count, 1, 65535)} />
    </div>
  );

  const save = async () => {
    try {
      await api.setSettings(s);
      onSaved(s);
      onClose();
    } catch (e) {
      // 설정은 저장됐지만 단축키 등록만 실패한 경우도 여기로 온다
      onSaved(s);
      setError(errText(e));
    }
  };

  return (
    <Modal title="설정" onClose={onClose} onOk={save} okText="저장" wide>
      <nav className="tabs">
        {TABS.map((t) => (
          <button key={t} className={t === tab ? "on" : ""} onClick={() => setTab(t)}>
            {t}
          </button>
        ))}
      </nav>
      <div className="tab-body">
        {tab === "일반" && (
          <>
            <div className="row">
              <label className="field grow">
                <span>저장 폴더</span>
                <input className="path" value={s.outputDir} onChange={(e) => set("outputDir", e.target.value)} />
              </label>
              <button
                onClick={async () => {
                  const d = await api.pickFolder(s.outputDir);
                  if (d) set("outputDir", d);
                }}
              >
                찾아보기…
              </button>
            </div>
            <label className="field">
              <span>파일 이름 규칙</span>
              <input value={s.fileNameRule} onChange={(e) => set("fileNameRule", e.target.value)} />
            </label>
            <p className="hint">%Y 년, %m 월, %d 일, %H 시, %M 분, %S 초</p>
            <Check label="녹화 틀을 항상 위에" checked={s.alwaysOnTop} onChange={(v) => set("alwaysOnTop", v)} />
            <Check label="파일을 지울 때 휴지통으로" checked={s.deleteToRecycleBin} onChange={(v) => set("deleteToRecycleBin", v)} />
            <Check label="켤 때 새 버전 확인 (GitHub에 접속)" checked={s.checkUpdates} onChange={(v) => set("checkUpdates", v)} />
            <Check
              label="작업 자동 저장 (비정상 종료 시 복구)"
              checked={s.autoRecovery}
              onChange={(v) => set("autoRecovery", v)}
            />
            <p className="hint">편집 중인 프레임이 PC에 임시로 남습니다. 정상적으로 끄면 지웁니다.</p>
            <Check
              label="저장하지 않은 녹화·편집 내용을 버릴 때 묻지 않기"
              checked={s.doNotAskDiscardRecording}
              onChange={(v) => set("doNotAskDiscardRecording", v)}
            />
            <Check
              label="50 FPS 넘는 GIF 저장 경고 안 함"
              checked={s.doNotAskGif50fps}
              onChange={(v) => set("doNotAskGif50fps", v)}
            />
            <h3>설정 파일</h3>
            <div className="row">
              <button
                onClick={async () => {
                  try {
                    const p = await api.pickSaveFile("opencam-settings.json", [["OpenCam 설정", ["json"]]]);
                    if (!p) return;
                    await api.exportSettings(p, s);
                    setError(null);
                    alertAsk("모든 환경 설정을 내보냈습니다.");
                  } catch (e) {
                    setError(errText(e));
                  }
                }}
              >
                모든 환경 설정 내보내기
              </button>
              <button
                onClick={async () => {
                  try {
                    const p = await api.pickFile([["OpenCam 설정", ["json"]]]);
                    if (!p) return;
                    const next = await api.importSettings(p);
                    setS(next);
                    onSaved(next);
                    setError(null);
                    alertAsk("설정 정보를 가져왔습니다.");
                  } catch (e) {
                    setError(errText(e));
                  }
                }}
              >
                환경 설정 가져오기
              </button>
            </div>
          </>
        )}
        {tab === "녹화" && (
          <>
            <NumField label="기본 FPS" value={s.fps} min={1} max={100} onChange={num("fps", 1, 100)} />
            <div className="row">
              <NumField
                label="최대 녹화 시간"
                value={s.maxRecSec}
                min={1}
                max={3600}
                suffix="초"
                disabled={s.recNoTimeLimit}
                onChange={num("maxRecSec", 1, 3600)}
              />
              <Check label="시간 제한 없음" checked={s.recNoTimeLimit} onChange={(v) => set("recNoTimeLimit", v)} />
            </div>
            <NumField
              label="편집 메모리 한도"
              value={s.maxEditMemoryMb}
              min={64}
              max={65536}
              suffix="MB"
              onChange={num("maxEditMemoryMb", 64, 65536)}
            />
            <Check label="화면 변화 없으면 프레임 합치기" checked={s.skipSameFrame} onChange={(v) => set("skipSameFrame", v)} />
            <Check
              label="녹화 중 다른 창에서도 Ctrl+S로 화면 저장"
              checked={s.recGlobalSnapshotHotkey}
              onChange={(v) => set("recGlobalSnapshotHotkey", v)}
            />
            <p className="hint">켜면 녹화 중 Ctrl+S가 OpenCam에 전달되어 다른 프로그램의 저장 단축키가 동작하지 않습니다.</p>
            <Check
              label="녹화가 끝나면 저장 창 없이 바로 편집하기"
              checked={s.gotoEditAfterRecording}
              onChange={(v) => set("gotoEditAfterRecording", v)}
            />
            <h3>MP4 바로 저장</h3>
            <Check
              label="녹화 중 MP4로 바로 저장하기 (소리 없음)"
              checked={s.recDirectSave}
              onChange={(v) => set("recDirectSave", v)}
            />
            <div className="group">
              <NumField
                label="MP4 저장 품질 (추천값: 90)"
                value={s.recDirectSaveQuality}
                min={1}
                max={100}
                disabled={!s.recDirectSave}
                onChange={num("recDirectSaveQuality", 1, 100)}
              />
              <Check
                label="저장 후 폴더 열기"
                checked={s.recDirectSaveOpenFolder}
                disabled={!s.recDirectSave}
                onChange={(v) => set("recDirectSaveOpenFolder", v)}
              />
              <Check
                label="메모리 허용량을 모두 쓰면 편집용 프레임은 버리고 계속 녹화 (성능 향상)"
                checked={s.recDirectSaveDropFrames}
                disabled={!s.recDirectSave}
                onChange={(v) => set("recDirectSaveDropFrames", v)}
              />
              <Check
                label="바로 저장할 때는 녹화 시간 제한 없음"
                checked={s.recDirectSaveNoTimeLimit}
                disabled={!s.recDirectSave}
                onChange={(v) => set("recDirectSaveNoTimeLimit", v)}
              />
            </div>
            <h3>마우스 커서</h3>
            <Check label="커서 녹화" checked={s.captureCursor} onChange={(v) => set("captureCursor", v)} />
            <div className="row">
              <Check label="커서 강조" checked={s.cursorHighlight} onChange={(v) => set("cursorHighlight", v)} />
              {color("색", "cursorHighlightColor")}
            </div>
            <div className="row">
              <Check label="클릭 효과" checked={s.cursorClickFx} onChange={(v) => set("cursorClickFx", v)} />
              {color("왼쪽", "leftClickColor")}
              {color("오른쪽", "rightClickColor")}
            </div>
            <h3>녹화 틀</h3>
            <Check label="이동 및 크기 조절 중 녹화 틀을 반투명하게" checked={s.useSemiTransparency} onChange={(v) => set("useSemiTransparency", v)} />
            <Check label="크기 조절 중 돋보기 사용" checked={s.useMagnifier} onChange={(v) => set("useMagnifier", v)} />
            <Check label="툴바를 아래에" checked={s.recToolbarBottom} onChange={(v) => set("recToolbarBottom", v)} />
            <Check label="방향키 이동 가속" checked={s.recArrowKeyAccel} onChange={(v) => set("recArrowKeyAccel", v)} />
            <Check
              label="녹화 중에는 방향키 무시"
              checked={s.ignoreArrowKeysWhileRecording}
              onChange={(v) => set("ignoreArrowKeysWhileRecording", v)}
            />
          </>
        )}
        {tab === "편집" && (
          <>
            <Check label="되돌리기 사용" checked={s.enableUndo} onChange={(v) => set("enableUndo", v)} />
            <Check
              label="프레임 시간을 소수점 3째 자리까지 보여주기"
              checked={s.frameTime3digit}
              onChange={(v) => set("frameTime3digit", v)}
            />
            <NumField
              label="JPG 내보내기 화질"
              value={s.jpgExportQuality}
              min={1}
              max={100}
              onChange={num("jpgExportQuality", 1, 100)}
            />
            <h3>프레임 줄이기 기본값</h3>
            <NumField label="유사도" value={s.reduceSimilarity} min={0} max={50} suffix="%" onChange={num("reduceSimilarity", 0, 50)} />
            <NumField label="색 차이 허용치" value={s.reduceTolerance} min={0} max={30} onChange={num("reduceTolerance", 0, 30)} />
            <div className="row">
              <Check label="시간 제한" checked={s.reduceTimeLimit} onChange={(v) => set("reduceTimeLimit", v)} />
              <NumField
                label=""
                value={s.reduceTimeLimitMs}
                min={10}
                suffix="ms"
                disabled={!s.reduceTimeLimit}
                onChange={num("reduceTimeLimitMs", 10, 600000)}
              />
            </div>
            <Check label="재생 속도 유지" checked={s.reduceKeepSpeed} onChange={(v) => set("reduceKeepSpeed", v)} />
            <h3>재생 진행 막대</h3>
            <div className="row">
              {color("막대", "embedProgressColor")}
              {color("배경", "embedProgressBgColor")}
              <NumField
                label="두께"
                value={s.embedProgressLineWidth}
                min={1}
                max={50}
                suffix="px"
                onChange={num("embedProgressLineWidth", 1, 50)}
              />
              <NumField
                label="최소 재생시간"
                value={s.embedProgressMinSec}
                min={0}
                suffix="초"
                onChange={num("embedProgressMinSec", 0, 3600)}
              />
            </div>
          </>
        )}
        {tab === "GIF" && (
          <>
            <NumField label="화질" value={s.gifQuality} min={1} max={100} onChange={num("gifQuality", 1, 100)} />
            <Check label="디더링 (색 띠를 점으로 섞음, 용량이 조금 늘어남)" checked={s.gifDither} onChange={(v) => set("gifDither", v)} />
            <Check label="비슷한 픽셀 합치기" checked={s.gifRemoveSimilarPixels} onChange={(v) => set("gifRemoveSimilarPixels", v)} />
            {loop("gifFiniteLoop", "gifLoopCount")}
          </>
        )}
        {tab === "WebP" && (
          <>
            <NumField label="화질" value={s.webpQuality} min={1} max={100} onChange={num("webpQuality", 1, 100)} />
            <NumField label="압축 방식 (0 빠름 ~ 6 작게)" value={s.webpMethod} min={0} max={6} onChange={num("webpMethod", 0, 6)} />
            <Check label="무손실" checked={s.webpLossless} onChange={(v) => set("webpLossless", v)} />
            <Check label="아이폰 호환모드" checked={s.webpIphoneCompat} onChange={(v) => set("webpIphoneCompat", v)} />
            {loop("webpFiniteLoop", "webpLoopCount")}
          </>
        )}
        {tab === "APNG" && (
          <>
            <div className="radio-list">
              <label className="check">
                <input type="radio" checked={s.apng256Colors} onChange={() => set("apng256Colors", true)} />
                최적화된 256색으로 저장 (작은 파일)
              </label>
              <label className="check">
                <input type="radio" checked={!s.apng256Colors} onChange={() => set("apng256Colors", false)} />
                24비트 컬러로 저장 (색 그대로)
              </label>
            </div>
            {loop("apngFiniteLoop", "apngLoopCount")}
          </>
        )}
        {tab === "동영상" && (
          <>
            <NumField label="MP4 화질" value={s.mp4Quality} min={1} max={100} onChange={num("mp4Quality", 1, 100)} />
            <NumField label="WebM 화질" value={s.webmQuality} min={1} max={100} onChange={num("webmQuality", 1, 100)} />
            <NumField label="AVIF 화질" value={s.avifQuality} min={1} max={100} onChange={num("avifQuality", 1, 100)} />
            <div className="row">
              <Check
                label="MP4 투명 부분을 다른 색으로 바꾸기 (끄면 검정)"
                checked={s.mp4ReplaceTransparent}
                onChange={(v) => set("mp4ReplaceTransparent", v)}
              />
              {color("", "transparentReplaceColor")}
            </div>
            <p className="hint">MP4·WebM·AVIF 저장에는 ffmpeg가 필요합니다.</p>
          </>
        )}
        {tab === "단축키" && (
          <>
            <label className="field">
              <span>녹화 / 정지</span>
              <input value={s.hotkeyRecord} onChange={(e) => set("hotkeyRecord", e.target.value)} />
            </label>
            <label className="field">
              <span>일시정지</span>
              <input value={s.hotkeyPause} onChange={(e) => set("hotkeyPause", e.target.value)} />
            </label>
            <Check label="Ctrl+PrtSc로 녹화 / 정지" checked={s.hotkeyCtrlPrtsc} onChange={(v) => set("hotkeyCtrlPrtsc", v)} />
            <p className="hint">켜면 Ctrl+PrtSc가 다른 프로그램 대신 OpenCam에 전달될 수 있습니다.</p>
            <p className="hint">예: F2, Ctrl+Shift+R, Alt+F9. 비워 두면 단축키를 쓰지 않습니다.</p>
          </>
        )}
        {tab === "정보" && (
          <>
            <h3>OpenCam {version}</h3>
            <p>화면 녹화·GIF 편집 프로그램</p>
            <p className="hint">Copyright (C) 2026 JJ · GPL-3.0-or-later</p>
            <p className="hint">함께 쓰는 부품의 라이선스는 설치 폴더의 THIRD_PARTY_LICENSES.txt에 있습니다.</p>
            <div className="row">
              <button
                disabled={checking}
                onClick={async () => {
                  setChecking(true);
                  await checkForUpdate(true);
                  setChecking(false);
                }}
              >
                {checking ? "확인하는 중…" : "새 버전 확인"}
              </button>
              <button onClick={() => openUrl(REPO_URL)}>GitHub 페이지</button>
            </div>
          </>
        )}
      </div>
      {error && <p className="error">{error}</p>}
    </Modal>
  );
}
