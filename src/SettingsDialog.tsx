// 설정 창: 왼쪽 탭 목록 + 오른쪽 그 탭의 설정(도구 패널과 같은 부품). 형식별 저장 옵션은 저장 창에 있다.

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, bgrToHex, errText, hexToBgr, Settings } from "./api";
import { Check, Modal } from "./dialogs";
import { alertAsk } from "./ask";
import { checkForUpdate, REPO_URL } from "./updater";
import { ColorInput, Field, NumInput, Section } from "./Panel";

/** [탭, 한 줄 설명] */
const TABS = [
  ["일반", "저장 폴더 · 파일 이름"],
  ["녹화", "FPS · 커서 · 녹화 틀"],
  ["편집", "되돌리기 · 프레임 줄이기"],
  ["단축키", "녹화 · 일시정지"],
  ["정보", "버전 · 새 버전 확인"],
] as const;
type Tab = (typeof TABS)[number][0];

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
  // 색 설정은 BGR 숫자로 저장된다
  const color = (label: string, k: keyof Settings, disabled?: boolean) => (
    <ColorInput label={label} value={bgrToHex(s[k] as number)} disabled={disabled} onChange={(v) => set(k, hexToBgr(v) as never)} />
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
    <Modal title="설정" onClose={onClose} className="pane-modal">
      <div className="pane-grid">
        <div className="pane-nav" role="tablist" aria-label="설정 종류">
          {TABS.map(([t, desc]) => (
            <button key={t} role="tab" aria-selected={t === tab} className={t === tab ? "on" : ""} onClick={() => setTab(t)}>
              <span>{t}</span>
              <small>{desc}</small>
            </button>
          ))}
        </div>

        <div className="pane-right">
          <div className="pane-body settings-body">
            {tab === "일반" && (
              <>
                <Section title="저장">
                  <Field label="저장 폴더">
                    <input className="path" value={s.outputDir} onChange={(e) => set("outputDir", e.target.value)} />
                    <button
                      onClick={async () => {
                        const d = await api.pickFolder(s.outputDir);
                        if (d) set("outputDir", d);
                      }}
                    >
                      찾아보기…
                    </button>
                  </Field>
                  <Field label="파일 이름 규칙">
                    <input className="path" value={s.fileNameRule} onChange={(e) => set("fileNameRule", e.target.value)} />
                  </Field>
                  <p className="hint">%Y 년, %m 월, %d 일, %H 시, %M 분, %S 초</p>
                </Section>
                <Section title="동작">
                  <Check label="녹화 틀을 항상 위에" checked={s.alwaysOnTop} onChange={(v) => set("alwaysOnTop", v)} />
                  <Check label="파일을 지울 때 휴지통으로" checked={s.deleteToRecycleBin} onChange={(v) => set("deleteToRecycleBin", v)} />
                  <Check label="켤 때 새 버전 확인 (GitHub에 접속)" checked={s.checkUpdates} onChange={(v) => set("checkUpdates", v)} />
                  <Check label="작업 자동 저장 (비정상 종료 시 복구)" checked={s.autoRecovery} onChange={(v) => set("autoRecovery", v)} />
                  <p className="hint">편집 중인 프레임이 PC에 임시로 남습니다. 정상적으로 끄면 지웁니다.</p>
                  <Check
                    label="저장하지 않은 녹화·편집 내용을 버릴 때 묻지 않기"
                    checked={s.doNotAskDiscardRecording}
                    onChange={(v) => set("doNotAskDiscardRecording", v)}
                  />
                  <Check label="50 FPS 넘는 GIF 저장 경고 안 함" checked={s.doNotAskGif50fps} onChange={(v) => set("doNotAskGif50fps", v)} />
                </Section>
                <Section title="설정 파일">
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
                </Section>
              </>
            )}

            {tab === "녹화" && (
              <>
                <Section title="녹화">
                  <Field label="기본 FPS">
                    <NumInput value={s.fps} min={1} max={100} unit="FPS" label="기본 FPS" onChange={num("fps", 1, 100)} />
                  </Field>
                  <Field label="최대 녹화 시간">
                    <NumInput
                      value={s.maxRecSec}
                      min={1}
                      max={3600}
                      unit="초"
                      label="최대 녹화 시간"
                      disabled={s.recNoTimeLimit}
                      onChange={num("maxRecSec", 1, 3600)}
                    />
                    <Check label="제한 없음" checked={s.recNoTimeLimit} onChange={(v) => set("recNoTimeLimit", v)} />
                  </Field>
                  <Field label="편집 메모리 한도">
                    <NumInput
                      value={s.maxEditMemoryMb}
                      min={64}
                      max={65536}
                      unit="MB"
                      label="편집 메모리 한도"
                      wide
                      onChange={num("maxEditMemoryMb", 64, 65536)}
                    />
                  </Field>
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
                </Section>
                <Section title="MP4 바로 저장">
                  <Check label="녹화 중 MP4로 바로 저장하기 (소리 없음)" checked={s.recDirectSave} onChange={(v) => set("recDirectSave", v)} />
                  <Field label="품질" sub>
                    <NumInput
                      value={s.recDirectSaveQuality}
                      min={1}
                      max={100}
                      label="MP4 저장 품질"
                      disabled={!s.recDirectSave}
                      onChange={num("recDirectSaveQuality", 1, 100)}
                    />
                    <span className="hint">추천값 90</span>
                  </Field>
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
                </Section>
                <Section title="마우스 커서">
                  <Check label="커서 녹화" checked={s.captureCursor} onChange={(v) => set("captureCursor", v)} />
                  <Field label="커서 강조">
                    <Check label="" checked={s.cursorHighlight} onChange={(v) => set("cursorHighlight", v)} />
                    {color("커서 강조 색", "cursorHighlightColor", !s.cursorHighlight)}
                  </Field>
                  <Field label="클릭 효과">
                    <Check label="" checked={s.cursorClickFx} onChange={(v) => set("cursorClickFx", v)} />
                    {color("왼쪽 클릭 색", "leftClickColor", !s.cursorClickFx)}
                    {color("오른쪽 클릭 색", "rightClickColor", !s.cursorClickFx)}
                  </Field>
                  <p className="hint">클릭 효과 색: 왼쪽 버튼 · 오른쪽 버튼</p>
                </Section>
                <Section title="녹화 틀">
                  <Check
                    label="이동 및 크기 조절 중 녹화 틀을 반투명하게"
                    checked={s.useSemiTransparency}
                    onChange={(v) => set("useSemiTransparency", v)}
                  />
                  <Check label="크기 조절 중 돋보기 사용" checked={s.useMagnifier} onChange={(v) => set("useMagnifier", v)} />
                  <Check label="툴바를 아래에" checked={s.recToolbarBottom} onChange={(v) => set("recToolbarBottom", v)} />
                  <Check label="방향키 이동 가속" checked={s.recArrowKeyAccel} onChange={(v) => set("recArrowKeyAccel", v)} />
                  <Check
                    label="녹화 중에는 방향키 무시"
                    checked={s.ignoreArrowKeysWhileRecording}
                    onChange={(v) => set("ignoreArrowKeysWhileRecording", v)}
                  />
                </Section>
              </>
            )}

            {tab === "편집" && (
              <>
                <Section title="편집">
                  <Check label="되돌리기 사용" checked={s.enableUndo} onChange={(v) => set("enableUndo", v)} />
                  <Check
                    label="프레임 시간을 소수점 3째 자리까지 보여주기"
                    checked={s.frameTime3digit}
                    onChange={(v) => set("frameTime3digit", v)}
                  />
                  <Field label="JPG 내보내기 화질">
                    <NumInput value={s.jpgExportQuality} min={1} max={100} label="JPG 내보내기 화질" onChange={num("jpgExportQuality", 1, 100)} />
                  </Field>
                </Section>
                <Section title="프레임 줄이기 기본값">
                  <Field label="유사도">
                    <NumInput value={s.reduceSimilarity} min={0} max={50} unit="%" label="유사도" onChange={num("reduceSimilarity", 0, 50)} />
                  </Field>
                  <Field label="색 차이 허용치">
                    <NumInput value={s.reduceTolerance} min={0} max={30} label="색 차이 허용치" onChange={num("reduceTolerance", 0, 30)} />
                  </Field>
                  <Field label="시간 제한">
                    <Check label="" checked={s.reduceTimeLimit} onChange={(v) => set("reduceTimeLimit", v)} />
                    <NumInput
                      value={s.reduceTimeLimitMs}
                      min={10}
                      unit="ms"
                      label="시간 제한"
                      wide
                      disabled={!s.reduceTimeLimit}
                      onChange={num("reduceTimeLimitMs", 10, 600000)}
                    />
                  </Field>
                  <Check label="재생 속도 유지" checked={s.reduceKeepSpeed} onChange={(v) => set("reduceKeepSpeed", v)} />
                </Section>
                <Section title="재생 진행 막대">
                  <Field label="막대 색">{color("진행 막대 색", "embedProgressColor")}</Field>
                  <Field label="배경 색">{color("진행 막대 배경 색", "embedProgressBgColor")}</Field>
                  <Field label="두께">
                    <NumInput
                      value={s.embedProgressLineWidth}
                      min={1}
                      max={50}
                      unit="px"
                      label="두께"
                      onChange={num("embedProgressLineWidth", 1, 50)}
                    />
                  </Field>
                  <Field label="최소 재생시간">
                    <NumInput value={s.embedProgressMinSec} min={0} unit="초" label="최소 재생시간" onChange={num("embedProgressMinSec", 0, 3600)} />
                  </Field>
                </Section>
              </>
            )}

            {tab === "단축키" && (
              <Section title="단축키">
                <Field label="녹화 / 정지">
                  <input className="path" value={s.hotkeyRecord} onChange={(e) => set("hotkeyRecord", e.target.value)} />
                </Field>
                <Field label="일시정지">
                  <input className="path" value={s.hotkeyPause} onChange={(e) => set("hotkeyPause", e.target.value)} />
                </Field>
                <p className="hint">예: F2, Ctrl+Shift+R, Alt+F9. 비워 두면 단축키를 쓰지 않습니다.</p>
                <Check label="Ctrl+PrtSc로 녹화 / 정지" checked={s.hotkeyCtrlPrtsc} onChange={(v) => set("hotkeyCtrlPrtsc", v)} />
                <p className="hint">켜면 Ctrl+PrtSc가 다른 프로그램 대신 OpenCam에 전달될 수 있습니다.</p>
              </Section>
            )}

            {tab === "정보" && (
              <Section title={`OpenCam ${version}`}>
                <p className="hint">화면 녹화·GIF 편집 프로그램</p>
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
              </Section>
            )}
          </div>
        </div>
      </div>

      <div className="pane-foot">
        {error && <p className="error">{error}</p>}
        <div className="pane-foot-row">
          <span className="grow" />
          <button onClick={onClose}>취소</button>
          <button className="primary" onClick={save}>
            저장
          </button>
        </div>
      </div>
    </Modal>
  );
}
