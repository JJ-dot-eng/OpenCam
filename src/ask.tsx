// 앱 안의 확인·입력·알림 창.
// 이 앱의 웹뷰에서는 window.confirm이 창을 띄우지 않고 바로 "예"를 돌려줘서(지울지 묻는 곳이 그냥 지워짐),
// 모든 확인·입력은 이 함수들로 한다. 화면에 <AskHost />가 하나 있어야 한다.

import { useEffect, useState } from "react";
import { Modal } from "./dialogs";

type Request =
  | { kind: "confirm"; message: string; okText?: string; resolve: (v: boolean) => void }
  | { kind: "prompt"; message: string; value: string; resolve: (v: string | null) => void }
  | { kind: "alert"; message: string; resolve: () => void }
  | { kind: "choose"; message: string; choices: string[]; resolve: (v: number | null) => void };

let push: ((r: Request) => void) | null = null;

/** 예/아니요를 묻는다. 창을 띄울 수 없으면 안전하게 "아니요". */
export function confirmAsk(message: string, okText = "확인"): Promise<boolean> {
  return new Promise((resolve) => (push ? push({ kind: "confirm", message, okText, resolve }) : resolve(false)));
}

/** 글자를 입력받는다. 취소하면 null. */
export function promptAsk(message: string, value = ""): Promise<string | null> {
  return new Promise((resolve) => (push ? push({ kind: "prompt", message, value, resolve }) : resolve(null)));
}

export function alertAsk(message: string): Promise<void> {
  return new Promise((resolve) => (push ? push({ kind: "alert", message, resolve }) : resolve()));
}

/** 여러 버튼 중 하나를 고르게 한다. 고른 버튼의 번호, 취소하면 null. 첫 버튼이 추천(강조). */
export function chooseAsk(message: string, choices: string[]): Promise<number | null> {
  return new Promise((resolve) => (push ? push({ kind: "choose", message, choices, resolve }) : resolve(null)));
}

export function AskHost() {
  const [queue, setQueue] = useState<Request[]>([]);
  const [text, setText] = useState("");

  useEffect(() => {
    push = (r) => {
      setQueue((q) => [...q, r]);
      if (r.kind === "prompt") setText(r.value);
    };
    return () => {
      push = null;
    };
  }, []);

  const cur = queue[0];
  if (!cur) return null;
  const done = () => setQueue((q) => q.slice(1));
  const cancel = () => {
    if (cur.kind === "confirm") cur.resolve(false);
    else if (cur.kind === "prompt" || cur.kind === "choose") cur.resolve(null);
    else cur.resolve();
    done();
  };

  if (cur.kind === "choose") {
    const pick = (i: number) => {
      cur.resolve(i);
      done();
    };
    return (
      <div className="ask-layer">
        <Modal title="확인" onClose={cancel}>
          <p className="ask-message">{cur.message}</p>
          <div className="modal-foot">
            <button onClick={cancel}>취소</button>
            {cur.choices
              .map((c, i) => (
                <button key={i} className={i === 0 ? "primary" : ""} autoFocus={i === 0} onClick={() => pick(i)}>
                  {c}
                </button>
              ))
              .reverse()}
          </div>
        </Modal>
      </div>
    );
  }
  const ok = () => {
    if (cur.kind === "confirm") cur.resolve(true);
    else if (cur.kind === "prompt") cur.resolve(text);
    else cur.resolve();
    done();
  };

  return (
    <div className="ask-layer">
      <Modal
        title={cur.kind === "confirm" ? "확인" : cur.kind === "prompt" ? "입력" : "알림"}
        onClose={cancel}
        onOk={ok}
        okText={cur.kind === "confirm" ? cur.okText : "확인"}
        hideCancel={cur.kind === "alert"}
      >
        <p className="ask-message">{cur.message}</p>
        {cur.kind === "prompt" && (
          <input className="ask-input" autoFocus value={text} onChange={(e) => setText(e.target.value)} />
        )}
      </Modal>
    </div>
  );
}
