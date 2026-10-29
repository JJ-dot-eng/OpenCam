// 편집 창 오른쪽 패널. 도구 화면(자르기·필터·그리기·삽입·줌)은 미리보기 자리에 그리고,
// 설정 패널만 이 자리로 옮겨 그린다(포털). 그래서 패널이 프레임 목록 옆까지 길게 이어진다.
// 모양: 머리(도구 이름·짧은 설명) / 본문(섹션, 길면 여기만 스크롤) / 바닥(적용 범위·결과·취소·실행, 늘 보임)

import { createContext, ReactNode, useContext } from "react";
import { createPortal } from "react-dom";

export const SidePanelTarget = createContext<HTMLElement | null>(null);

export default function SidePanel({
  title,
  desc,
  foot,
  children,
}: {
  title: string;
  desc?: ReactNode;
  foot?: ReactNode;
  children: ReactNode;
}) {
  const target = useContext(SidePanelTarget);
  if (!target) return null;
  return createPortal(
    <div className="tool-side">
      <header className="side-head">
        <h2 className="side-title">{title}</h2>
        {desc && <p className="side-desc">{desc}</p>}
      </header>
      <div className="side-body">{children}</div>
      {foot && <footer className="side-foot">{foot}</footer>}
    </div>,
    target,
  );
}
