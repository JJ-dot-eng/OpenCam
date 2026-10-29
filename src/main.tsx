import React, { Component, lazy, ReactNode, Suspense } from "react";
import ReactDOM from "react-dom/client";

// 창마다 같은 index.html을 쓰고, 주소 뒤 #으로 화면을 고른다 (tauri.conf.json의 url).
// 따로 불러와야 각 화면의 CSS가 다른 창에 섞이지 않는다.
const Screen = window.location.hash === "#recorder" ? lazy(() => import("./Recorder"))
  : window.location.hash === "#selector" ? lazy(() => import("./AreaSelector"))
  : lazy(() => import("./Editor"));

// 기본 오른쪽 클릭 메뉴(새로 고침 등)는 막는다
window.addEventListener("contextmenu", (e) => {
  if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
});

/** 화면 코드에서 오류가 나도 하얀 창 대신 내용을 보여준다. 편집 중인 프레임은 Rust 쪽에 남아 있다. */
class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div style={{ padding: 24, font: "14px 'Segoe UI', 'Malgun Gothic', sans-serif", background: "#fff", color: "#222" }}>
        <h2 style={{ margin: "0 0 8px" }}>화면에 오류가 났습니다</h2>
        <p>편집 중인 내용은 그대로 남아 있습니다. 다시 불러오면 이어서 작업할 수 있습니다.</p>
        <pre style={{ whiteSpace: "pre-wrap", color: "#b91c1c" }}>{error.message}</pre>
        <button onClick={() => window.location.reload()}>다시 불러오기</button>
      </div>
    );
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <Suspense fallback={null}>
        <Screen />
      </Suspense>
    </ErrorBoundary>
  </React.StrictMode>,
);
