# 인계

갱신: 2026-10-31, Codex 담당 (OpenCam 담당 위임)

자세한 기록은 [docs/TODO.md](docs/TODO.md).

## 끝난 일

- PR #4·#5를 main에 합치고 0.1.7로 릴리스함.
- PR #6(GIF 투명 잔상)·#7(붙이기 겹침·작업 변경)·#8(동영상 일부만 읽음 경고·길이 없는 동영상)을 main에 합치고 0.1.8로 릴리스함.
- GitHub Actions CI가 PR마다 Windows에서 타입 검사·노드 테스트·화면 빌드·Rust 빌드·cargo test를 돌린다.
- PR #10(`test/e2e-media`, 열림·아직 안 합침): 창 없는 Rust 미디어 e2e 테스트 8개 추가. PNG·투명 PNG·투명 GIF·MP4·길이 정보 없는 WebM·끝이 잘린 AVI 열기 → 붙이기 → GIF·WebP·APNG·MP4·WebM·PNG 연속 저장 → 다시 읽기를 확인한다. Windows CI에서 cargo test 112 → 120개 통과, 무시한 테스트 없음. ffmpeg가 없으면 CI 실패(`OPENCAM_REQUIRE_FFMPEG`). 실제 앱에서만 확인할 항목은 [docs/real-app-checklist.md](docs/real-app-checklist.md)에 적었다.
- PR #9(`test/ui-logic`, 열림·아직 안 합침): 끌어 놓기 처리 판단·붙이기 잠금 노드 테스트 추가(13 → 49개). 동작을 바꾸지 않고 `Editor.tsx`의 순수 로직을 `src/dropHandling.ts`·`src/appendLock.ts`로 옮겼다.
- PR #9·#10 모두 CI 통과. 테스트에서 새 버그는 찾지 못했다.

## 남은 일

- PR #9·#10을 main에 합칠지 사용자가 정한다.
- Windows PC에서 [docs/real-app-checklist.md](docs/real-app-checklist.md) 항목을 확인한다. 세로 모니터 녹화, 복구 카드의 복구 동작, 0.1.7의 GIF·PNG 저장도 실제 앱 확인이 남아 있다.
- 정적 검토 버그 3단계: #12 편집이 프로젝트 잠금을 쥔 채 rayon을 기다리고 썸네일 요청(`frame_png`)이 rayon 안에서 잠금을 기다려 멈출 수 있음(의심, `lib.rs`·`project.rs`).
- 낮음 후보: APNG 프레임 시간 65.5초 잘림, GIF 시간 보정이 APNG·WebP에도 적용됨, 시간 합 u32 넘침, 클립보드 삽입 이미지 Blob URL 미해제, 동영상 붙이기 메모리 한도 계산 오차, 진행률 99% 정체.
- 보류한 버그: 정전 시 복구 파일 동기화, 녹화 틀 옮길 때 잠깐 끊김, 저장 결과에 단축키 오류.
- AGENTS.md의 Rust 테스트 수(75)는 오래되었다(PR #10 포함 120개). 사용자 허락 없이 고치지 않음.

## 사용자가 정한 것
- 새 기능은 바로 추가하지 않고 직접 사용하거나 사용자 반응을 본 뒤 정한다.
- 할 일과 기획 문서는 공개 저장소 docs에 둔다. 여러 AI는 AGENTS.md의 협업 규칙을 따른다.
- 코드 서명은 하지 않는다.
- 끌어 놓은 동영상 여러 개도 파일 이름 순서(숫자는 크기대로)로 맨 뒤에 붙인다. 편집 중이 아니면 기존처럼 연다.
- 새 브랜치와 PR로만 올리고 main에 직접 push·합치기·강제 push 하지 않는다.
