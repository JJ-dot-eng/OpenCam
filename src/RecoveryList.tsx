// 빈 편집 화면의 "복구할 작업" 목록: 비정상 종료로 남은 작업을 다시 열거나 버린다.

import { IconRestore, IconX } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { confirmAsk } from "./ask";
import { api, errText, formatMs, RecoveryEntry } from "./api";

/** 저장 시각을 "오늘 오후 3:12"·"어제 오후 3:12"·"10월 28일 오후 3:12"로 */
function when(ms: number): string {
  const d = new Date(ms);
  const time = d.toLocaleTimeString("ko-KR", { hour: "numeric", minute: "2-digit" });
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(new Date()) - day(d)) / 86400000);
  if (diff === 0) return `오늘 ${time}`;
  if (diff === 1) return `어제 ${time}`;
  return `${d.getMonth() + 1}월 ${d.getDate()}일 ${time}`;
}

function Thumb({ id }: { id: string }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let made: string | null = null;
    let alive = true;
    api
      .recoveryThumb(id)
      .then((buf) => {
        if (!alive) return;
        made = URL.createObjectURL(new Blob([buf], { type: "image/png" }));
        setUrl(made);
      })
      .catch(() => {});
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [id]);
  return <div className="rec-thumb">{url && <img src={url} alt="" />}</div>;
}

export default function RecoveryList({
  busy,
  onOpen,
  onError,
}: {
  /** 다른 작업(복구 포함)을 처리 중이면 버튼을 막는다. 복구를 겹쳐 누르면 복구본이 둘 다 지워진다. */
  busy: boolean;
  onOpen: (id: string) => void;
  onError: (m: string) => void;
}) {
  const [items, setItems] = useState<RecoveryEntry[]>([]);
  const refresh = useCallback(() => {
    api.recoveryList().then(setItems, () => setItems([]));
  }, []);
  useEffect(refresh, [refresh]);

  if (items.length === 0) return null;
  const discard = async (e: RecoveryEntry) => {
    if (!(await confirmAsk(`${when(e.savedAt)} 작업을 버릴까요? 되살릴 수 없습니다.`, "버리기"))) return;
    try {
      await api.recoveryDiscard(e.id);
    } catch (err) {
      onError(errText(err));
    }
    refresh();
  };
  return (
    <section className="rec-list">
      <h3>저장하지 못하고 꺼진 작업</h3>
      {items.map((e) => (
        <div className="rec-item" key={e.id}>
          <Thumb id={e.id} />
          <div className="rec-info">
            <b>{when(e.savedAt)}</b>
            <span>
              {e.frames}장 · {formatMs(e.totalMs)} · {e.width}×{e.height}
            </span>
          </div>
          <button className="primary" disabled={busy} onClick={() => onOpen(e.id)} title="이 작업을 편집 창으로 불러옵니다">
            <IconRestore size={18} />
            복구
          </button>
          <button className="ghost icon" disabled={busy} onClick={() => discard(e)} title="버리기">
            <IconX size={18} />
          </button>
        </div>
      ))}
    </section>
  );
}
